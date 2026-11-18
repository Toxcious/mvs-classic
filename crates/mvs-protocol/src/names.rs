//! A player's name (the words of the bad-word list): its length and the words it may not hold,
//! the port's own rules (the build's names are the backend's). The relay checks a presence's name by them.

use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

/// The build's name modal takes 50 characters (UI_TextInput's CharacterLimit); the port takes 20 (UTF-16 units).
pub const NAME_LIMIT: usize = 20;

/// Control and format characters (the bidirectional overrides among them): a name shown to another player holds none.
/// The zero width joiner (U+200D) and the tag characters (U+E0020-E007F) stay, as emoji sequences are built of them.
/// Unverified: the regex crate's Unicode tables are taken to class Cc / Cf as JavaScript's engine's do.
static UNSHOWN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[[\p{Cc}\p{Cf}\u{2028}\u{2029}]--[\u{200D}\u{E0020}-\u{E007F}]]").expect("UNSHOWN"));

/// JavaScript's `\s`.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// The head of `s` of at most `units` UTF-16 units. A cut inside a surrogate pair drops the pair.
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

/// `nameShowable`: true when the name holds no control or format character.
pub fn name_showable(name: &str) -> bool {
    !UNSHOWN.is_match(name)
}

/// `cleanName`: a name another player sent, as it is shown: without control and format characters, whitespace runs
/// as one space, trimmed and cut to [`NAME_LIMIT`]; '' when nothing is left.
pub fn clean_name(v: &str) -> String {
    let spaced: String = utf16_head(v, NAME_LIMIT * 8).chars().map(|c| if js_space(c) { ' ' } else { c }).collect();
    let shown = UNSHOWN.replace_all(&spaced, "");
    let mut one = String::with_capacity(shown.len());
    for c in shown.chars() {
        if !(c == ' ' && one.ends_with(' ')) {
            one.push(c);
        }
    }
    utf16_head(one.trim_matches(' '), NAME_LIMIT).trim_matches(' ').to_string()
}

/// `WordList`.
#[derive(Clone, Copy, Debug)]
pub struct WordList {
    /// Refused wherever they stand in a word.
    pub inside: &'static [&'static str],
    /// Refused as a word of their own (with a plural s).
    pub whole: &'static [&'static str],
    /// Innocent words that hold one of `inside`.
    pub allowed: &'static [&'static str],
}

/// `BAD_WORDS`: the port's own list, written for it: English, the common swear words and slurs. Add a word in lower
/// case, plain letters: `inside` only terms few innocent words hold; `whole` short terms that innocent words hold
/// ("class", "grape", "cockpit", "Dickens", "Montenegro", "Nazir").
pub const BAD_WORDS: WordList = WordList {
    inside: &[
        "fuck",
        "fck",
        "phuck",
        "shit",
        "cunt",
        "bitch",
        "biatch",
        "whore",
        "slut",
        "asshole",
        "arsehole",
        "bastard",
        "wanker",
        "bollocks",
        "pussy",
        "penis",
        "vagina",
        "dildo",
        "jizz",
        "porn",
        "cocksuck",
        "dickhead",
        "nigg",
        "faggot",
        "tranny",
        "retard",
        "wetback",
        "hitler",
        "pedophil",
        "paedophil",
        "molest",
        "killyourself",
    ],
    whole: &[
        "ass", "arse", "cock", "dick", "prick", "twat", "wank", "cum", "tit", "anal", "anus", "sex", "piss", "slag",
        "fuk", "fuq", "nazi", "rape", "raper", "rapist", "pedo", "paedo", "nonce", "fag", "homo", "kike", "spic",
        "chink", "paki", "coon", "gook", "beaner", "negro", "kkk", "kys",
    ],
    allowed: &[
        "scunthorpe",
        "mishit",
        "mishits",
        "shitake",
        "shiitake",
        "matsushita",
        "snigger",
        "sniggers",
        "sniggered",
        "sniggering",
        "niggle",
        "niggles",
        "niggled",
        "niggling",
        "penistone",
        "pussycat",
        "pussyfoot",
        "pussywillow",
        "retardant",
        "yoshito",
        "yoshitaka",
        "yoshitomo",
        "toshito",
        "toshitaka",
        "ishita",
        "shital",
        "shitala",
    ],
};

/// A word of the list as a pattern: each letter any number of times, and a letter the word doubles at least twice
/// ("ass" is not "as", "fuuuck" is "fuck").
fn pattern(word: &str) -> String {
    let b = word.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let mut j = i + 1;
        while j < b.len() && b[j] == c {
            j += 1;
        }
        let run = j - i;
        if c.is_ascii_lowercase() {
            if run > 1 {
                out.push_str(&format!("{}{{{run},}}", c as char));
            } else {
                out.push_str(&format!("{}+", c as char));
            }
        } else {
            out.push_str(&regex::escape(&word[i..j]));
        }
        i = j;
    }
    out
}

/// A word with its runs of one letter as one.
fn squeeze(w: &str) -> String {
    let mut out = String::with_capacity(w.len());
    for c in w.chars() {
        if !(c.is_ascii_lowercase() && out.ends_with(c)) {
            out.push(c);
        }
    }
    out
}

/// `profanity`: the check of a name against a list: true when the name is refused.
/// A name is read in lower case, without its accents and through the swaps ("0" o, "3" e, "4" a, "5" s, "7" t, "@" a,
/// "$" s; "1", "!" and "|" both as i and as l); what is not a letter then parts its words. A word is refused when it
/// holds an `inside` term (unless it is an `allowed` word) or is a `whole` term. Letters set apart are read together as
/// well ("f.u.c.k", "s h i t", "fu ck"): a single letter joins the word beside it, and so do two neighbours of at most
/// two letters. Longer words are not joined, so that two innocent words are not read as one ("push it").
#[derive(Clone, Debug)]
pub struct Profanity {
    inside: Regex,
    whole: Regex,
    allowed: Vec<String>,
}

impl Profanity {
    pub fn new(list: &WordList) -> Profanity {
        let inside = list.inside.iter().map(|w| pattern(w)).collect::<Vec<_>>().join("|");
        let whole = format!("^(?:{})(?:e?s)?$", list.whole.iter().map(|w| pattern(w)).collect::<Vec<_>>().join("|"));
        Profanity {
            inside: Regex::new(&inside).expect("profanity inside"),
            whole: Regex::new(&whole).expect("profanity whole"),
            allowed: list.allowed.iter().map(|w| squeeze(w)).collect(),
        }
    }

    fn bad(&self, w: &str) -> bool {
        self.whole.is_match(w) || (self.inside.is_match(w) && !self.allowed.contains(&squeeze(w)))
    }

    fn read(&self, text: &str) -> bool {
        let words: Vec<&str> = text.split(|c: char| !c.is_ascii_lowercase()).filter(|w| !w.is_empty()).collect();
        if words.iter().any(|w| self.bad(w)) {
            return true;
        }
        // The run of words read together, and the length of its last one.
        let mut joined = String::new();
        let mut last = 0;
        for w in words {
            if !joined.is_empty() && (w.len() == 1 || last == 1 || (w.len() <= 2 && last <= 2)) {
                joined.push_str(w);
            } else {
                joined = w.to_string();
            }
            last = w.len();
            if joined != w && self.bad(&joined) {
                return true;
            }
        }
        false
    }

    /// Whether the name is refused.
    pub fn refuses(&self, name: &str) -> bool {
        let plain: String = name
            .nfkd()
            .filter(|c| !('\u{300}'..='\u{36F}').contains(c))
            .collect::<String>()
            .to_lowercase()
            .chars()
            .map(|c| match c {
                '0' => 'o',
                '3' => 'e',
                '4' => 'a',
                '5' => 's',
                '7' => 't',
                '@' => 'a',
                '$' => 's',
                c => c,
            })
            .collect();
        let as_ =
            |to: char| plain.chars().map(|c| if matches!(c, '1' | '!' | '|') { to } else { c }).collect::<String>();
        self.read(&as_('i')) || self.read(&as_('l'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean() {
        assert_eq!(clean_name("  Long\u{200b}  name xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"), "Long name xxxxxxxxxx");
        assert_eq!(clean_name(" \u{202e} "), "");
        assert_eq!(clean_name("a\u{200d}b\tc"), "a\u{200d}b c");
        assert!(name_showable("ab") && !name_showable("a\u{202e}"));
    }

    #[test]
    fn words() {
        let p = Profanity::new(&BAD_WORDS);
        for bad in ["f.u.c.k", "s h i t", "fu ck", "FUUUCK", "a55", "sh1t", "kkks", "fück"] {
            assert!(p.refuses(bad), "{bad}");
        }
        for ok in ["Player", "push it", "Scunthorpe", "class", "as", "Dickens", "Montenegro"] {
            assert!(!p.refuses(ok), "{ok}");
        }
    }
}
