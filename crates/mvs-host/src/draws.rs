//! A queue match's random choices from every human's draw (queue.join `draw`): the core's own rule,
//! the build's queue was its backend's matchmaker. The draws are sorted and hashed into a key that feeds a
//! counter-mode generator (SHA-256 of the key and a block counter), so a change of any one draw changes every choice
//! and no draw picks one alone.
//! Unverified as a guard: the core runs in the host's net process, which sees the guests' draws before the match is
//! found, so a host that runs a changed core can still choose its own draw after them.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// A draw is a whole number in [0, DRAW_LIMIT).
pub const DRAW_LIMIT: u64 = 1 << 32;

/// A join's draw as the game sent it; None: none, or not a whole number in [0, DRAW_LIMIT) (drawOf).
pub fn draw_of(v: &Value) -> Option<u32> {
    let x = v.as_f64()?;
    if x.fract() != 0.0 || x < 0.0 || x >= 4_294_967_296.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = x as u32;
    Some(n)
}

/// The generator of a match whose humans drew these: the same draws in any order give the same choices
/// (combinedDraws).
#[derive(Clone, Debug)]
pub struct CombinedDraws {
    key: [u8; 32],
    block: u32,
    words: [u8; 32],
    word: usize,
}

impl CombinedDraws {
    pub fn new(draws: &[u32]) -> CombinedDraws {
        let mut sorted = draws.to_vec();
        sorted.sort_unstable();
        let mut h = Sha256::new();
        h.update(b"mvs-draws");
        for d in sorted {
            h.update(d.to_be_bytes());
        }
        CombinedDraws { key: h.finalize().into(), block: 0, words: [0; 32], word: 8 }
    }

    fn next(&mut self) -> u32 {
        if self.word == 8 {
            let mut h = Sha256::new();
            h.update(self.key);
            h.update(self.block.to_be_bytes());
            self.block = self.block.wrapping_add(1);
            self.words = h.finalize().into();
            self.word = 0;
        }
        let at = 4 * self.word;
        self.word += 1;
        u32::from_be_bytes([self.words[at], self.words[at + 1], self.words[at + 2], self.words[at + 3]])
    }

    /// A whole number in [0, n), n in [1, DRAW_LIMIT] (0 for n under 2, with nothing drawn). Panics past DRAW_LIMIT
    /// (a range error).
    pub fn int(&mut self, n: u64) -> u32 {
        if n < 2 {
            return 0;
        }
        assert!(n <= DRAW_LIMIT, "CombinedDraws::int: a draw of more than 2^32 values ({n})");
        // (the values past the last whole multiple of n are drawn again, so that each of the n is as likely)
        let limit = DRAW_LIMIT - (DRAW_LIMIT % n);
        loop {
            let x = u64::from(self.next());
            if x < limit {
                return u32::try_from(x % n).expect("below n, which is at most 2^32");
            }
        }
    }
}
