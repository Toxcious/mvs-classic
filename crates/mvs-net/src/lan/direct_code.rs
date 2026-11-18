//! A direct host's lobby code: what a player on the host's
//! network pastes in Join Lobby.
//!   d + base32 (RFC 4648 alphabet, lowercase, no padding) of
//!     version   1 byte ([`DIRECT_CODE_VERSION`])
//!     count     1 byte, 1 or 2
//!     addresses count x 4 bytes: the host's IPv4 addresses on its network, the likeliest first ([`pick_addresses`]);
//!               a reader takes only an address of a local network ([`Local`]), so a code never points a game
//!               outside its network
//!     port      2 bytes, big-endian: TCP and UDP
//!     key       8 bytes: the first 8 of the SHA-256 of the host's raw public key (the key's fingerprint)
//!     lobby     10 bytes: the host core's lobby id, 'l' + these 20 hex digits
//!     check     2 bytes: the first 2 of the SHA-256 of every byte before, so a mistyped code fails cleanly
//! 46 characters with one address, 53 with two (the join box takes 64). The joiner proves the key at each address
//! before it signs in, and the lobby id is what lets it in: a player on the network who knows neither cannot join.
//! Pure: no I/O. A refusal is a [`DirectWhy`], not words: the menus word it.

use sha2::{Digest, Sha256};

pub const DIRECT_CODE_VERSION: u8 = 1;
/// The most addresses a code holds (a PC on wired and Wi-Fi at once).
pub const DIRECT_ADDRESSES: usize = 2;

/// A direct host's lobby, as its code holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectCode {
    /// Dotted IPv4, the likeliest first.
    pub addresses: Vec<String>,
    pub port: u16,
    /// 16 hex digits.
    pub fingerprint: String,
    /// The host core's lobby id: 'l' + 20 hex digits.
    pub lobby: String,
}

/// Why a text is not a direct code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectWhy {
    /// A character is wrong, missing or extra.
    Mistyped,
    /// Another version (the number) made the code: one of the two apps needs an update.
    Version(u8),
    /// The code names an address outside a local network.
    Outside,
}

impl DirectWhy {
    /// The log's words, for the log and the vectors alone; the menus word a refusal themselves.
    pub fn log_text(&self) -> String {
        match self {
            DirectWhy::Mistyped => "that direct code is mistyped (a character is wrong, missing or extra)".to_owned(),
            DirectWhy::Version(v) => {
                format!("that direct code is of another version ({v}): one of the two apps needs an update")
            }
            DirectWhy::Outside => "that direct code names an address outside a local network".to_owned(),
        }
    }
}

/// Why a code cannot be written (a programming error: the host's own values). The words are the log's.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EncodeError {
    #[error("a direct code cannot hold the address {0}")]
    Address(String),
    #[error("a direct code needs an address")]
    NoAddress,
    #[error("a direct code cannot hold the port {0}")]
    Port(f64),
    #[error("a direct code needs a 16-digit key fingerprint")]
    Fingerprint,
    #[error("a direct code needs a lobby id of l and 20 hex digits")]
    Lobby,
}

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const KEY_BYTES: usize = 8;
const LOBBY_BYTES: usize = 10;
const CHECK_BYTES: usize = 2;

/// Lowercase base32 (RFC 4648 alphabet, no padding) of `b`.
pub fn base32(b: &[u8]) -> String {
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
pub fn unbase32(s: &str) -> Option<Vec<u8>> {
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

fn check(b: &[u8]) -> [u8; CHECK_BYTES] {
    let sum = Sha256::digest(b);
    [sum[0], sum[1]]
}

/// JavaScript's `\s`.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// A code's text as the join path compares it: no spaces, lowercase.
pub fn direct_text(s: &str) -> String {
    s.split(js_space).collect::<String>().to_lowercase()
}

/// The four bytes of a dotted IPv4 address (each part one to three ASCII digits, read as decimal: "010" is 10), or
/// None.
fn ipv4(a: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = a.split('.');
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        *slot = u8::try_from(p.parse::<u16>().ok()?).ok()?;
    }
    parts.next().is_none().then_some(out)
}

fn is_private(b: &[u8; 4]) -> bool {
    b[0] == 10 || (b[0] == 172 && (16..=31).contains(&b[1])) || (b[0] == 192 && b[1] == 168)
}

fn is_link_local(b: &[u8; 4]) -> bool {
    b[0] == 169 && b[1] == 254
}

/// An address of a local network: private (10/8, 172.16/12, 192.168/16) or link-local (169.254/16); loopback (127/8)
/// only when allowed (tests). Nothing else is ever written in a code or read from one, so a code (an invite carries
/// one) never makes a player's game connect out of its network: it cannot be used to learn a player's internet
/// address.
fn usable(b: &[u8; 4], loopback: bool) -> bool {
    is_private(b) || is_link_local(b) || (loopback && b[0] == 127)
}

/// Carrier-grade NAT's shared range 100.64/10, which Tailscale gives its devices.
fn is_cgnat(b: &[u8; 4]) -> bool {
    b[0] == 100 && (64..=127).contains(&b[1])
}

/// A unicast IPv4 address a host may hold: not 0/8, loopback (127/8, unless `loopback`), multicast or the reserved
/// 240/4. The encoder's guard of the host's own values; which of them is written is [`pick_addresses`]'s rule.
fn writable(b: &[u8; 4], loopback: bool) -> bool {
    b[0] != 0 && b[0] < 224 && (b[0] != 127 || loopback)
}

/// The leading ones of a netmask, None when its ones are not contiguous.
fn prefix_of(mask: &[u8; 4]) -> Option<u32> {
    let m = u32::from_be_bytes(*mask);
    let n = m.leading_ones();
    (m.checked_shl(n).unwrap_or(0) == 0).then_some(n)
}

/// The smallest network (the longest prefix is /8) of this PC's own a peer's address may be in: an interface's
/// netmask shorter than it (a /4 a misconfigured adapter gives) adds nothing.
pub const OWN_NET_PREFIX_MIN: u32 = 8;

/// What counts as a local network here: the private and link-local ranges (`usable`), and the own networks of this PC
/// that a code may name ([`Local::of`]: a virtual network adapter's, Radmin VPN's 26/8, Hamachi's 25/8, a ZeroTier
/// network, and 100.64/10, Tailscale's), each a prefix of [`OWN_NET_PREFIX_MIN`] or longer; whether loopback (127/8)
/// is let through (tests).
/// No address outside them is ever read from a code or let in by the direct listener: a code (an invite carries one)
/// cannot make a game connect out to the internet, nor learn a player's internet address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Local {
    pub nets: Vec<([u8; 4], [u8; 4])>,
    pub loopback: bool,
}

impl Local {
    /// No own network known: the private and link-local ranges alone (and loopback with `loopback`).
    pub fn new(loopback: bool) -> Local {
        Local { nets: Vec::new(), loopback }
    }

    /// The own networks of `nets`' external interfaces a code may name, by the rule of [`pick_addresses`]: every
    /// network of a virtual network's adapter (`is_virtual_network`), and an address's in 100.64/10 on any adapter,
    /// within 100.64/10. A plain adapter's other networks add nothing: their private and link-local addresses are
    /// local already, and a public one (a phone hotspot's 31.4.5.6/8) would make a piece of the internet local.
    pub fn of(nets: &Interfaces, loopback: bool) -> Local {
        const CGNAT: ([u8; 4], [u8; 4]) = ([100, 64, 0, 0], [255, 192, 0, 0]);
        let mut out: Vec<([u8; 4], [u8; 4])> = Vec::new();
        for (name, list) in nets {
            let vpn = is_virtual_network(name);
            for x in list.iter().filter(|x| x.v4 && !x.internal) {
                let (Some(a), Some(m)) = (ipv4(&x.address), ipv4(&x.netmask)) else { continue };
                let net = if is_cgnat(&a) {
                    // Unverified: the netmask Tailscale's adapter reports on Windows (a /32 or its /10): on its
                    // adapter the network is all of 100.64/10 whatever it says; on another, the reported one cut to it.
                    let tailscale = name.to_lowercase().contains("tailscale");
                    if tailscale || prefix_of(&m).is_none_or(|n| n < 10) { CGNAT } else { (a, m) }
                } else if vpn {
                    (a, m)
                } else {
                    continue;
                };
                if !out.contains(&net) {
                    out.push(net);
                }
            }
        }
        Local { nets: out, loopback }
    }

    /// `b` is of a local network: `usable`, or inside one of the own networks.
    pub fn takes(&self, b: &[u8; 4]) -> bool {
        let own = |(a, m): &([u8; 4], [u8; 4])| {
            let (a, m, x) = (u32::from_be_bytes(*a), u32::from_be_bytes(*m), u32::from_be_bytes(*b));
            prefix_of(&m.to_be_bytes()).is_some_and(|n| n >= OWN_NET_PREFIX_MIN) && a & m == x & m
        };
        usable(b, self.loopback) || (b[0] != 127 && writable(b, false) && self.nets.iter().any(own))
    }
}

/// Whether a peer's address (dotted IPv4, or IPv4 mapped in IPv6) is of a local network as [`Local::takes`] says: the
/// direct listener's filter of the connections and datagrams it takes.
pub fn local_address(address: &str, local: &Local) -> bool {
    let plain = if address.len() >= 7 && address.is_char_boundary(7) && address[..7].eq_ignore_ascii_case("::ffff:") {
        &address[7..]
    } else {
        address
    };
    ipv4(plain).is_some_and(|b| local.takes(&b))
}

/// An interface name of a virtual adapter (a VM's host-only network, WSL, a VPN): its address is rarely the one a
/// friend on the same network reaches. Unverified: the port's own list of the usual names on Windows.
const VIRTUAL: [&str; 12] = [
    "vmware",
    "virtualbox",
    "vbox",
    "vethernet",
    "hyper-v",
    "wsl",
    "docker",
    "loopback",
    "tailscale",
    "zerotier",
    "hamachi",
    "vpn",
];

fn is_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    VIRTUAL.iter().any(|v| n.contains(v)) || n.contains("tap-") || n.contains("tun")
}

/// One address of an interface (os.networkInterfaces()'s fields read).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nic {
    pub address: String,
    pub netmask: String,
    /// IPv4 ('IPv4' or 4).
    pub v4: bool,
    pub internal: bool,
}

/// This PC's interfaces by name, in the order the system lists them.
pub type Interfaces = Vec<(String, Vec<Nic>)>;

/// A virtual network's adapter (Tailscale, ZeroTier, Radmin VPN, Hamachi) by its name: the one kind of adapter whose
/// address outside the private ranges is written in a code. Unverified: the names these tools give their adapters on
/// Windows (Tailscale "Tailscale", ZeroTier "ZeroTier One [<network>]", Radmin "Radmin VPN", Hamachi "Hamachi").
const VIRTUAL_NETWORKS: [&str; 4] = ["tailscale", "zerotier", "radmin", "hamachi"];

fn is_virtual_network(name: &str) -> bool {
    let n = name.to_lowercase();
    VIRTUAL_NETWORKS.iter().any(|v| n.contains(v))
}

/// This PC's IPv4 addresses a friend on its network may reach, the likeliest first: a physical adapter's private
/// address, then a virtual network adapter's address (`is_virtual_network`: Tailscale, ZeroTier, Radmin, Hamachi) or
/// one in 100.64/10, then another virtual adapter's private address (a VM's, WSL's, Docker's), then link-local (a
/// physical adapter's first). A VPN adapter's comes before a VM's, so with the LAN's it takes the code's two slots.
/// Loopback only with `loopback` (tests: the host listens on
/// 127.0.0.1 alone, and that is the one address).
///
/// The reader takes an address of its own
/// networks too ([`Local`]), so this pick is the one guard of what a code tells: never a public address of a plain
/// adapter (a PC on the internet directly, a phone's hotspot), which would give the player's internet address to
/// whoever reads an invite.
pub fn pick_addresses(nets: &Interfaces, loopback: bool) -> Vec<String> {
    if loopback {
        return vec!["127.0.0.1".to_owned()];
    }
    let mut found: Vec<(String, u8)> = Vec::new();
    for (name, list) in nets {
        for x in list {
            if !x.v4 || x.internal {
                continue;
            }
            let Some(b) = ipv4(&x.address) else { continue };
            let vpn = is_virtual_network(name);
            let rank = if is_private(&b) && !is_virtual(name) {
                0
            } else if is_cgnat(&b) || (vpn && writable(&b, false)) {
                1
            } else if is_private(&b) {
                2
            } else if is_link_local(&b) {
                3 + u8::from(is_virtual(name))
            } else {
                continue;
            };
            found.push((x.address.clone(), rank));
        }
    }
    found.sort_by_key(|f| f.1);
    let mut seen: Vec<String> = Vec::new();
    for (a, _) in found {
        if !seen.contains(&a) {
            seen.push(a);
        }
    }
    seen
}

/// A JavaScript `Number.isInteger` port in 1..=65535, as the code writes it.
pub fn port_of(port: f64) -> Result<u16, EncodeError> {
    if port.fract() == 0.0 && (1.0..=65535.0).contains(&port) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "an integer within 1..=65535")]
        return Ok(port as u16);
    }
    Err(EncodeError::Port(port))
}

fn hex_digits(s: &str, n: usize) -> Option<Vec<u8>> {
    (s.len() == n && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        .then(|| (0..n / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap_or(0)).collect())
}

/// The code of a lobby; errs on a field it cannot hold (a programming error: the host's own values). `loopback`: 127.x
/// may be written (tests).
///
/// This takes any unicast address
/// (`writable`: a virtual network's 26/8 or 25/8 is one), [`pick_addresses`] choosing which of the host's are written.
pub fn encode_direct_code(c: &DirectCode, loopback: bool) -> Result<String, EncodeError> {
    let mut addrs: Vec<[u8; 4]> = Vec::new();
    for a in c.addresses.iter().take(DIRECT_ADDRESSES) {
        match ipv4(a) {
            Some(b) if writable(&b, loopback) => addrs.push(b),
            _ => return Err(EncodeError::Address(a.clone())),
        }
    }
    if addrs.is_empty() {
        return Err(EncodeError::NoAddress);
    }
    if c.port < 1 {
        return Err(EncodeError::Port(f64::from(c.port)));
    }
    let fingerprint = hex_digits(&c.fingerprint, 16).ok_or(EncodeError::Fingerprint)?;
    let lobby = c.lobby.strip_prefix('l').and_then(|h| hex_digits(h, 20)).ok_or(EncodeError::Lobby)?;
    let mut b = vec![DIRECT_CODE_VERSION, u8::try_from(addrs.len()).unwrap_or(0)];
    for a in &addrs {
        b.extend_from_slice(a);
    }
    b.extend_from_slice(&c.port.to_be_bytes());
    b.extend_from_slice(&fingerprint);
    b.extend_from_slice(&lobby);
    let sum = check(&b);
    b.extend_from_slice(&sum);
    Ok(format!("d{}", base32(&b)))
}

/// The lengths a code has (one or two addresses).
fn lengths() -> [usize; 2] {
    [1usize, 2].map(|k| 1 + ((2 + k * 4 + 2 + KEY_BYTES + LOBBY_BYTES + CHECK_BYTES) * 8).div_ceil(5))
}

/// Whether a join code is of a direct host by its form: `d` and base32 of about a code's length. An EOS lobby id (32
/// hex digits) or a relay's ('l' + 20 hex) never is; a direct code with a character missing or added still is, so it
/// fails as mistyped rather than as a lobby of another backend.
pub fn looks_direct(s: &str) -> bool {
    let t = direct_text(s);
    let l = lengths();
    let n = t.encode_utf16().count();
    t.strip_prefix('d').is_some_and(|r| !r.is_empty() && r.bytes().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7')))
        && n + 6 >= l[0]
        && n <= l[1] + 6
}

/// A code read back: its fields, or why it is not one; an address outside `local`'s networks is refused.
pub fn decode_direct_code(s: &str, local: &Local) -> Result<DirectCode, DirectWhy> {
    let t = direct_text(s);
    if !t.starts_with('d') || !lengths().contains(&t.encode_utf16().count()) {
        return Err(DirectWhy::Mistyped);
    }
    let b = unbase32(&t[1..]).filter(|b| b.len() >= 2).ok_or(DirectWhy::Mistyped)?;
    let n = b.len() - CHECK_BYTES;
    let sum = check(&b[..n]);
    if sum[0] != b[n] || sum[1] != b[n + 1] {
        return Err(DirectWhy::Mistyped);
    }
    if b[0] != DIRECT_CODE_VERSION {
        return Err(DirectWhy::Version(b[0]));
    }
    let count = usize::from(b[1]);
    if !(1..=DIRECT_ADDRESSES).contains(&count) || n != 2 + count * 4 + 2 + KEY_BYTES + LOBBY_BYTES {
        return Err(DirectWhy::Mistyped);
    }
    let mut addresses = Vec::new();
    for i in 0..count {
        let a: [u8; 4] = b[2 + i * 4..6 + i * 4].try_into().expect("four bytes");
        // (an address of a network this reader is not on is left out: a host on its LAN and on Radmin is reached by a
        // friend on either)
        if local.takes(&a) {
            addresses.push(format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]));
        }
    }
    if addresses.is_empty() {
        return Err(DirectWhy::Outside);
    }
    let mut at = 2 + count * 4;
    let port = u16::from_be_bytes([b[at], b[at + 1]]);
    at += 2;
    if port < 1 {
        return Err(DirectWhy::Mistyped);
    }
    let hex = |x: &[u8]| x.iter().map(|v| format!("{v:02x}")).collect::<String>();
    let fingerprint = hex(&b[at..at + KEY_BYTES]);
    at += KEY_BYTES;
    let lobby = format!("l{}", hex(&b[at..at + LOBBY_BYTES]));
    Ok(DirectCode { addresses, port, fingerprint, lobby })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one() -> DirectCode {
        DirectCode {
            addresses: vec!["192.168.1.14".into()],
            port: 7777,
            fingerprint: "0123456789abcdef".into(),
            lobby: "l0123456789abcdef0123".into(),
        }
    }

    #[test]
    fn one_and_two_addresses_round_trip_at_their_lengths() {
        let c1 = encode_direct_code(&one(), false).unwrap();
        let two = DirectCode { addresses: vec!["192.168.1.14".into(), "10.0.0.5".into()], port: 65535, ..one() };
        let c2 = encode_direct_code(&two, false).unwrap();
        assert_eq!((c1.len(), c2.len()), (46, 53));
        assert_eq!(decode_direct_code(&c1, &Local::new(false)), Ok(one()));
        assert_eq!(decode_direct_code(&c2, &Local::new(false)), Ok(two));
    }

    #[test]
    fn every_one_character_change_is_refused() {
        let c = encode_direct_code(&one(), false).unwrap();
        for i in 1..c.len() {
            for ch in "abcdefghijklmnopqrstuvwxyz234567".chars() {
                if c.chars().nth(i) == Some(ch) {
                    continue;
                }
                let mut v: Vec<char> = c.chars().collect();
                v[i] = ch;
                assert!(
                    decode_direct_code(&v.into_iter().collect::<String>(), &Local::new(false)).is_err(),
                    "{i} {ch}"
                );
            }
        }
    }

    #[test]
    fn loopback_and_public_addresses() {
        let lo = DirectCode { addresses: vec!["127.0.0.1".into()], ..one() };
        assert!(encode_direct_code(&lo, false).is_err());
        let c = encode_direct_code(&lo, true).unwrap();
        assert_eq!(decode_direct_code(&c, &Local::new(false)), Err(DirectWhy::Outside));
        assert!(decode_direct_code(&c, &Local::new(true)).is_ok());
        let lan = Local::new(false);
        assert!(
            !local_address("8.8.8.8", &lan)
                && local_address("::ffff:192.168.0.9", &lan)
                && local_address("010.0.0.1", &lan)
        );
    }
}
