//! A lobby's code as the players paste it: what a player pastes in
//! Join Lobby, whatever backends they are on. One form, a kind letter, then lowercase base32 (RFC 4648 alphabet, no
//! padding) with a check, under the join box's 64 characters:
//!
//! - `e` an EOS lobby: version 1 byte; form 1 byte (0 the id is lowercase hex, its bytes follow; 1 the id is text, its
//!   UTF-8); the lobby id, EPIC_ID_MAX bytes at the most (EOS's 32 hex digits: 16); check 2 bytes. 33 characters for
//!   EOS's ids.
//! - `r` a lobby on a relay: version 1 byte; form 1 byte (0 the release's default relay, the short form; n (1 to
//!   RELAY_HOST_MAX) the relay's host name of n characters follows); port 2 bytes, big-endian (not in the short form);
//!   host the host name (or IP address, lowercase), three characters in two bytes (base 40) (not in the short form);
//!   lobby 10 bytes (the relay's lobby id, 'l' + these 20 hex digits); check 2 bytes. 24 characters in the short
//!   form; with a host name of RELAY_HOST_MAX (33) characters, 62.
//! - `d` a direct (LAN) host's lobby: the direct code's own form (46 or 53 characters), read here to its check only.
//! - (the port's own) a short code: SHORT_LEN characters of SHORT_ALPHABET, which the release's relay gives a lobby
//!   it hosts; no kind letter and no check (the relay's join limits hold the guesses). Every long form is longer, so
//!   any text of SHORT_LEN characters is read as one: of another character it is mistyped.
//!
//! The check of `e` and `r` is the first 2 bytes of the SHA-256 of the kind letter and every byte before it, so a
//! mistyped code fails as such (a letter changed among them too), never as a lobby of another kind. Pure: no I/O.
//!
//! The words of a refusal are the menus': [`CodeWhy`] names the reason.

use mvs_protocol::relay_wire::{SHORT_ALPHABET, SHORT_LEN, parse_address};
use sha2::{Digest, Sha256};

use crate::lan::direct_code::{DirectWhy, EncodeError, Local, decode_direct_code};

/// A lobby's code read: the kinds of lobby a join box takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyCode {
    Epic {
        lobby: String,
    },
    /// `relay` the relay's address as codes compare it ([`normal_relay`]); None: the release's default relay.
    Relay {
        lobby: String,
        relay: Option<String>,
    },
    /// The direct code's text.
    Direct {
        code: String,
    },
    /// (wire 6) A hosted lobby's short code on the release's default relay: SHORT_LEN characters of SHORT_ALPHABET,
    /// which the relay resolves to the lobby at the join ([`is_short`]).
    Short {
        code: String,
    },
}

/// The text (as the join path compares it: no spaces, lowercase) is a short code's form: SHORT_LEN characters, each
/// of SHORT_ALPHABET. Read before the kind letter: a short code may start with any of them.
pub fn is_short(t: &str) -> bool {
    t.len() == SHORT_LEN && t.bytes().all(|b| SHORT_ALPHABET.contains(&b))
}

/// Why a lobby's code cannot be written (a programming error: the host's own values). The menus word it; the
/// `Display` is the log's words.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum CodeMakeWhy {
    /// A direct code's text that is not one.
    #[error("a direct lobby code starts with d")]
    DirectKind,
    #[error("a lobby code cannot hold the EOS lobby id {lobby}")]
    EpicId { lobby: String },
    #[error("a lobby code cannot hold the relay lobby id {lobby}")]
    RelayId { lobby: String },
    /// A relay lobby's code without a relay, and this app has no default relay.
    #[error("a lobby code of the default relay needs one: this app has none")]
    NoDefaultRelay,
    #[error("a lobby code cannot hold the relay address {relay}")]
    RelayAddress { relay: String },
    /// A direct host's code (direct_code.rs EncodeError): an address it cannot hold.
    #[error("a direct code cannot hold the address {address}")]
    DirectAddress { address: String },
    #[error("a direct code needs an address")]
    DirectNoAddress,
    #[error("a direct code cannot hold the port {port}")]
    DirectPort { port: u16 },
    #[error("a direct code needs a 16-digit key fingerprint")]
    DirectFingerprint,
    #[error("a direct code needs a lobby id of l and 20 hex digits")]
    DirectLobby,
}

impl CodeMakeWhy {
    /// A direct code's why, of a code whose port is `port`.
    pub fn of_direct(e: EncodeError, port: u16) -> CodeMakeWhy {
        match e {
            EncodeError::Address(address) => CodeMakeWhy::DirectAddress { address },
            EncodeError::NoAddress => CodeMakeWhy::DirectNoAddress,
            EncodeError::Port(_) => CodeMakeWhy::DirectPort { port },
            EncodeError::Fingerprint => CodeMakeWhy::DirectFingerprint,
            EncodeError::Lobby => CodeMakeWhy::DirectLobby,
        }
    }
}

/// Why a text is not a lobby code (the join box words each; on the wire in a JoinWhy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeWhy {
    /// There is no lobby code.
    Empty,
    /// Not a kind this app can read: mistyped, or made by a newer version of the app.
    UnknownKind,
    /// A character is wrong, missing or extra.
    Mistyped,
    /// Another version (the number) made the code: one of the two apps needs an update.
    Version(u8),
    /// Not written the way its host writes it: copy it from the host again.
    Spelled,
    /// A direct code with a character wrong, missing or extra.
    DirectMistyped,
    /// A direct code of another version.
    DirectVersion(u8),
    /// A direct code that names an address outside a local network.
    DirectOutside,
}

pub const LOBBY_CODE_VERSION: u8 = 1;
/// The longest relay host name a code holds (any port with it): the code is then 62 characters.
pub const RELAY_HOST_MAX: usize = 33;
/// The longest EOS lobby id a code holds, in bytes (as text; hex ids are half that in the code).
pub const EPIC_ID_MAX: usize = 34;

const CHECK_BYTES: usize = 2;
const RELAY_LOBBY_BYTES: usize = 10;
/// A host name's characters, digit 0 the padding of its last group.
const HOST_DIGITS: &[u8] = b"\0abcdefghijklmnopqrstuvwxyz0123456789.-:";
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

/// JavaScript's \s.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// A code's text as the join path compares it: no spaces, lowercase.
fn direct_text(s: &str) -> String {
    s.split(js_space).collect::<String>().to_lowercase()
}

/// Lowercase base32 (RFC 4648 alphabet, no padding) of `b`.
fn base32(b: &[u8]) -> String {
    let mut out = String::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for x in b {
        acc = (acc << 8) | u32::from(*x);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

/// The bytes of `s` (lowercase base32, no padding); None when a character is not of the alphabet or the last one's
/// spare bits are not zero (so one code has one spelling).
fn unbase32(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for ch in s.chars() {
        let v = ALPHABET.iter().position(|a| char::from(*a) == ch)?;
        acc = (acc << 5) | u32::try_from(v).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 255).ok()?);
        }
        acc &= (1 << bits) - 1;
    }
    (acc == 0).then_some(out)
}

fn check(kind: &str, b: &[u8]) -> [u8; CHECK_BYTES] {
    let sum = Sha256::new().chain_update(kind.as_bytes()).chain_update(b).finalize();
    [sum[0], sum[1]]
}

/// A direct code's check, version and addresses (decodeDirectCode, its fields not kept): the LAN module's reader.
fn check_direct(t: &str) -> Result<(), CodeWhy> {
    match decode_direct_code(t, &Local::new(true)) {
        Ok(_) => Ok(()),
        Err(DirectWhy::Mistyped) => Err(CodeWhy::DirectMistyped),
        Err(DirectWhy::Version(v)) => Err(CodeWhy::DirectVersion(v)),
        Err(DirectWhy::Outside) => Err(CodeWhy::DirectOutside),
    }
}

// ---------- the lobby code ----------

fn sealed(kind: &str, body: &[u8]) -> String {
    let mut b = body.to_vec();
    b.extend_from_slice(&check(kind, body));
    format!("{kind}{}", base32(&b))
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

fn lower_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A relay address as codes compare it: `host:port` (`[v6]:port`), the host lowercase; None when it is none.
pub fn normal_relay(address: &str) -> Option<String> {
    let p = parse_address(address)?;
    let host = p.host.to_lowercase();
    Some(if host.contains(':') { format!("[{host}]:{}", p.port) } else { format!("{host}:{}", p.port) })
}

/// Two relay addresses name the same relay ([`normal_relay`]).
pub fn same_relay(a: Option<&str>, b: Option<&str>) -> bool {
    let (Some(a), Some(b)) = (a.filter(|a| !a.is_empty()), b.filter(|b| !b.is_empty())) else { return false };
    normal_relay(a).is_some_and(|x| Some(x) == normal_relay(b))
}

/// The code of a lobby; `default_relay` the release's default relay (a relay lobby on it gets the short form). Errs on
/// a field it cannot hold (a programming error: the host's own values).
pub fn encode_lobby_code(c: &LobbyCode, default_relay: Option<&str>) -> Result<String, CodeMakeWhy> {
    match c {
        LobbyCode::Direct { code } => {
            let t = direct_text(code);
            if !t.starts_with('d') {
                return Err(CodeMakeWhy::DirectKind);
            }
            Ok(t)
        }
        // (the relay's own code, written as it gave it)
        LobbyCode::Short { code } => Ok(direct_text(code)),
        LobbyCode::Epic { lobby } => {
            let hex = !lobby.is_empty() && lobby.len() % 2 == 0 && lower_hex(lobby);
            let id = if hex { unhex(lobby) } else { lobby.as_bytes().to_vec() };
            if id.is_empty() || id.len() > EPIC_ID_MAX {
                return Err(CodeMakeWhy::EpicId { lobby: lobby.clone() });
            }
            let mut body = vec![LOBBY_CODE_VERSION, u8::from(!hex)];
            body.extend_from_slice(&id);
            Ok(sealed("e", &body))
        }
        LobbyCode::Relay { lobby, relay } => {
            if !(lobby.len() == 21 && lobby.starts_with('l') && lower_hex(&lobby[1..])) {
                return Err(CodeMakeWhy::RelayId { lobby: lobby.clone() });
            }
            let id = unhex(&lobby[1..]);
            if relay.is_none() && default_relay.is_none_or(str::is_empty) {
                return Err(CodeMakeWhy::NoDefaultRelay);
            }
            if relay.is_none() || same_relay(relay.as_deref(), default_relay) {
                let mut body = vec![LOBBY_CODE_VERSION, 0];
                body.extend_from_slice(&id);
                return Ok(sealed("r", &body));
            }
            let relay = relay.as_deref().unwrap_or_default();
            let cannot = || CodeMakeWhy::RelayAddress { relay: relay.to_owned() };
            let p = parse_address(relay).ok_or_else(cannot)?;
            let host = p.host.to_lowercase();
            let host = host.as_bytes();
            if host.is_empty() || host.len() > RELAY_HOST_MAX || host.iter().any(|ch| !HOST_DIGITS[1..].contains(ch)) {
                return Err(cannot());
            }
            let mut packed: Vec<u8> = Vec::new();
            for g in host.chunks(3) {
                let d = |k: usize| g.get(k).map_or(0, |ch| HOST_DIGITS.iter().position(|h| h == ch).unwrap_or(0));
                let v = d(0) * 1600 + d(1) * 40 + d(2);
                packed.push(u8::try_from(v >> 8).unwrap_or(0));
                packed.push(u8::try_from(v & 255).unwrap_or(0));
            }
            let mut body = vec![LOBBY_CODE_VERSION, u8::try_from(host.len()).unwrap_or(0)];
            body.extend_from_slice(&p.port.to_be_bytes());
            body.extend_from_slice(&packed);
            body.extend_from_slice(&id);
            Ok(sealed("r", &body))
        }
    }
}

/// A code read back, or why it is not one (mistyped, another version, a kind this app does not know, or a spelling
/// the encoder would not write: an EOS id of hex digits as text, a relay's address in full where it is
/// `default_relay`, the release's default relay, whose code is the short form). The text is read as [`direct_text`]
/// does: no spaces, lowercase.
pub fn decode_lobby_code(s: &str, default_relay: Option<&str>) -> Result<LobbyCode, CodeWhy> {
    let t = direct_text(s);
    let code = read_lobby_code(&t)?;
    // (one spelling of a lobby: the host core takes the code its host wrote, so another spelling of the same lobby
    // would fail later, at the lobby's join)
    if matches!(code, LobbyCode::Direct { .. } | LobbyCode::Short { .. } | LobbyCode::Relay { relay: None, .. }) {
        return Ok(code);
    }
    let again = encode_lobby_code(&code, default_relay).unwrap_or_default();
    if again == t { Ok(code) } else { Err(CodeWhy::Spelled) }
}

fn read_lobby_code(t: &str) -> Result<LobbyCode, CodeWhy> {
    let kind = t.chars().next().ok_or(CodeWhy::Empty)?;
    // A text of six characters that reads as a short code is one (the lobby code's own short form).
    if is_short(t) {
        return Ok(LobbyCode::Short { code: t.to_owned() });
    }
    if t.chars().count() == SHORT_LEN {
        return Err(CodeWhy::Mistyped);
    }
    if kind == 'd' {
        // (the check and the version here; the join refuses an address it may not use)
        check_direct(t)?;
        return Ok(LobbyCode::Direct { code: t.to_owned() });
    }
    if kind != 'e' && kind != 'r' {
        return Err(CodeWhy::UnknownKind);
    }
    let kind_text = &t[..1];
    let b = unbase32(&t[1..]).ok_or(CodeWhy::Mistyped)?;
    // (one spelling of a code: a character more of spare bits alone is not the same code)
    if b.len() < 2 + CHECK_BYTES || (b.len() * 8).div_ceil(5) != t.len() - 1 {
        return Err(CodeWhy::Mistyped);
    }
    let n = b.len() - CHECK_BYTES;
    let sum = check(kind_text, &b[..n]);
    if sum[0] != b[n] || sum[1] != b[n + 1] {
        return Err(CodeWhy::Mistyped);
    }
    if b[0] != LOBBY_CODE_VERSION {
        return Err(CodeWhy::Version(b[0]));
    }
    let form = usize::from(b[1]);
    let body = &b[2..n];
    if kind == 'e' {
        if body.is_empty() || body.len() > EPIC_ID_MAX || form > 1 {
            return Err(CodeWhy::Mistyped);
        }
        if form == 0 {
            return Ok(LobbyCode::Epic { lobby: hex_of(body) });
        }
        // (TextDecoder drops a leading byte order mark)
        let text = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
        let lobby = std::str::from_utf8(text).map_err(|_| CodeWhy::Mistyped)?;
        return if lobby.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}') {
            Err(CodeWhy::Mistyped)
        } else {
            Ok(LobbyCode::Epic { lobby: lobby.to_owned() })
        };
    }
    let lobby_of = |x: &[u8]| format!("l{}", hex_of(x));
    if form == 0 {
        return if body.len() == RELAY_LOBBY_BYTES {
            Ok(LobbyCode::Relay { lobby: lobby_of(body), relay: None })
        } else {
            Err(CodeWhy::Mistyped)
        };
    }
    let groups = form.div_ceil(3);
    if form > RELAY_HOST_MAX || body.len() != 2 + groups * 2 + RELAY_LOBBY_BYTES {
        return Err(CodeWhy::Mistyped);
    }
    let port = u16::from(body[0]) << 8 | u16::from(body[1]);
    let mut host = String::new();
    for g in 0..groups {
        let v = usize::from(body[2 + g * 2]) << 8 | usize::from(body[3 + g * 2]);
        if v >= 64000 {
            return Err(CodeWhy::Mistyped);
        }
        let d = [v / 1600, v / 40 % 40, v % 40];
        for (k, digit) in d.iter().enumerate() {
            let at = g * 3 + k;
            // (a place within the name is a character, every one past it the padding)
            if (at < form) != (*digit > 0) {
                return Err(CodeWhy::Mistyped);
            }
            if at < form {
                host.push(char::from(HOST_DIGITS[*digit]));
            }
        }
    }
    let text = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    let relay = normal_relay(&text).filter(|_| port >= 1).ok_or(CodeWhy::Mistyped)?;
    Ok(LobbyCode::Relay { lobby: lobby_of(&body[2 + groups * 2..]), relay: Some(relay) })
}
