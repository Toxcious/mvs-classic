//! Text chat (the port's own: the beta has none): a match's team and all chat and the custom
//! lobby's chat. The limits and the cleaning of a message are shared by the game, which keeps to them before it
//! sends, and the host core, which drops what breaks them.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

/// A message's length at the most, in graphemes (an emoji is one).
pub const CHAT_LIMIT: usize = 80;
/// Messages a player may send at once, then one per 1 / CHAT_PER_SEC seconds.
pub const CHAT_BURST: f64 = 3.0;
pub const CHAT_PER_SEC: f64 = 0.5;
/// The emoji a message keeps at the most (the later ones go).
pub const CHAT_EMOJIS: usize = 6;
/// A bubble at a scoreboard portrait: fully shown this long, then fading out over CHAT_FADE_MS.
pub const CHAT_BUBBLE_MS: f64 = 4000.0;
pub const CHAT_FADE_MS: f64 = 500.0;
/// The bubbles at one portrait at the most (a newer one pushes the oldest out).
pub const CHAT_BUBBLES: usize = 2;

/// The combining marks kept on one grapheme at the most (more is "zalgo" text, drawn far above and below its line).
const MARKS_KEPT: usize = 3;
/// The pictographs one emoji may join (a family of four); a longer joined chain is not kept at all.
const EMOJI_PARTS: usize = 4;

/// Control and format characters (the bidirectional overrides among them) go; the zero width joiner (U+200D) and the
/// tag characters (U+E0020-E007F) stay, as emoji sequences are built of them.
static FORMAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[[\p{Cc}\p{Cf}\u{2028}\u{2029}]--[\u{200D}\u{E0020}-\u{E007F}]]").expect("FORMAT"));
/// The marks counted against MARKS_KEPT: the variation selectors and the keycap mark are parts of an emoji.
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{M}--[\u{FE0E}\u{FE0F}\u{20E3}]]").expect("MARK"));
/// An emoji's pictographs.
static PICTO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{Extended_Pictographic}").expect("PICTO"));
static REGIONAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{Regional_Indicator}").expect("REGIONAL"));

/// JavaScript's `\s` (also what `trim` strips).
pub(crate) fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// The head of `s` of at most `units` UTF-16 units. A cut inside a surrogate pair drops the pair
/// (a Rust string holds no lone high half).
fn utf16_head(s: &str, units: usize) -> &str {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        n += c.len_utf16();
        if n > units {
            return &s[..i];
        }
    }
    s
}

/// The pictographs a grapheme joins: each Extended_Pictographic one, a flag's two regional indicators as one
/// (`/\p{Extended_Pictographic}|\p{Regional_Indicator}{1,2}/gu`).
fn pictographs(g: &str) -> usize {
    let mut parts = 0;
    let mut chars = g.chars().peekable();
    let mut buf = [0u8; 4];
    while let Some(c) = chars.next() {
        if PICTO.is_match(c.encode_utf8(&mut buf)) {
            parts += 1;
        } else if REGIONAL.is_match(c.encode_utf8(&mut buf)) {
            parts += 1;
            if chars.peek().is_some_and(|n| REGIONAL.is_match(n.encode_utf8(&mut [0u8; 4]))) {
                chars.next();
            }
        }
    }
    parts
}

/// A message as it is sent and shown: whitespace runs as one space, without control and format characters, at most
/// a few marks on a grapheme and CHAT_EMOJIS emoji, trimmed and cut to CHAT_LIMIT graphemes; '' when nothing is left
/// or `v` is not a string (cleanChat).
pub fn clean_chat(v: &Value) -> String {
    let Some(v) = v.as_str() else { return String::new() };
    let spaced: String = utf16_head(v, CHAT_LIMIT * 16).chars().map(|c| if js_space(c) { ' ' } else { c }).collect();
    let shown = FORMAT.replace_all(&spaced, "");
    let mut one = String::with_capacity(shown.len());
    for c in shown.chars() {
        if !(c == ' ' && one.ends_with(' ')) {
            one.push(c);
        }
    }
    let s = one.trim_matches(js_space);
    let mut out = String::new();
    let mut count = 0;
    let mut emojis = 0;
    for g in s.graphemes(true) {
        if count == CHAT_LIMIT {
            break;
        }
        let parts = pictographs(g);
        if parts > 0 {
            if parts > EMOJI_PARTS {
                continue;
            }
            emojis += 1;
            if emojis > CHAT_EMOJIS {
                continue;
            }
        }
        let mut marks = 0;
        let mut buf = [0u8; 4];
        for ch in g.chars() {
            if MARK.is_match(ch.encode_utf8(&mut buf)) {
                marks += 1;
                if marks > MARKS_KEPT {
                    continue;
                }
            }
            out.push(ch);
        }
        count += 1;
    }
    out.trim_matches(js_space).to_owned()
}

/// The graphemes of a message, for the input's count (chatLength).
pub fn chat_length(s: &str) -> usize {
    s.graphemes(true).count()
}

/// A player's chat budget: CHAT_BURST at once, then CHAT_PER_SEC, each `times` over (chatGate). Times in ms.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatGate {
    burst: f64,
    per_sec: f64,
    tokens: f64,
    at: f64,
}

impl ChatGate {
    /// A full budget at `now`.
    pub fn new(times: f64, now: f64) -> ChatGate {
        let burst = CHAT_BURST * times;
        ChatGate { burst, per_sec: CHAT_PER_SEC * times, tokens: burst, at: now }
    }

    /// A send at `now`: taken (true) or dropped (false: nothing of the budget is spent).
    pub fn allow(&mut self, now: f64) -> bool {
        // Math.min's rule: a NaN anywhere makes NaN tokens, and `NaN < 1` is false, so such a gate takes every
        // send
        let t = self.tokens + ((now - self.at) / 1000.0) * self.per_sec;
        self.tokens = if t.is_nan() || self.burst.is_nan() { f64::NAN } else { self.burst.min(t) };
        self.at = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}
