//! The online config: which backends the net service signs
//! in to, all at once. The release's defaults are `online.json` beside the program (a fork changes that file, never
//! the code); the player's own choices in the launcher's Online settings are kept in `Settings.online` and win field
//! by field. No code names the port's relay or its EOS product: they are values of that file.
//!   online.json  { "relay": "host[:port]" | null, "relayId": "r<32 hex>" | null, "eos": true | false,
//!                  "lan": port | null, "direct": true | false }

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use mvs_protocol::relay_wire::{KnownRelay, is_key_id, parse_address};

/// The online settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OnlineConfig {
    /// The relay's address (`relay_wire::parse_address`); None: no relay.
    pub relay: Option<String>,
    /// The id of the key the release's relay proves (`relay_wire::key_id`): a relay at that address that proves
    /// another is refused. None: not pinned (a fork's or a dev config).
    #[serde(rename = "relayId", skip_serializing_if = "Option::is_none")]
    pub relay_id: Option<String>,
    /// Epic Online Services signed in to.
    pub eos: bool,
    /// The LAN port a hosted LAN lobby listens on (never LAN_DISCOVERY_PORT); None: no LAN.
    pub lan: Option<u16>,
    /// Friends in a custom game may connect directly (EOS relay control; the setting's line says they can then see
    /// this PC's address).
    pub direct: bool,
}

/// What the program uses when online.json is missing or a field of it is unreadable: no relay, Epic on, LAN off,
/// direct friend links off (both opt-in).
pub const ONLINE_DEFAULTS: OnlineConfig =
    OnlineConfig { relay: None, relay_id: None, eos: true, lan: None, direct: false };

impl Default for OnlineConfig {
    fn default() -> OnlineConfig {
        ONLINE_DEFAULTS
    }
}

/// The UDP port LAN lobbies are announced on and heard on (lan/announce.rs), fixed, not a setting: a LAN port equal
/// to it is refused (a hosted lobby's plain UDP socket and the listener's shared one cannot share a port). 39417: in
/// no well-known game's defaults (UE 7777, Source 27015, Minecraft 25565, Factorio 34197, Steam 27000-27050, STUN
/// 3478), below the 49152 where Windows hands out ephemeral ports, and not in Windows' services file.
// Unverified: not checked against the IANA registry itself.
pub const LAN_DISCOVERY_PORT: u16 = 39417;

/// The most relays a player's yes is kept for (the oldest dropped first).
const KNOWN_RELAYS_MAX: usize = 32;

/// The config's field names, in the file's order.
pub const ONLINE_KEYS: [&str; 5] = ["relay", "relayId", "eos", "lan", "direct"];

/// A port of JSON: a whole number from 1 to 65535.
fn port_of(v: &Value) -> Option<u16> {
    let n = v.as_f64()?;
    (n.fract() == 0.0 && (1.0..=65535.0).contains(&n)).then_some(n as u16)
}

/// A port the LAN setting may take: any but LAN_DISCOVERY_PORT.
pub fn lan_port_ok(v: &Value) -> Option<u16> {
    port_of(v).filter(|p| *p != LAN_DISCOVERY_PORT)
}

/// JavaScript's `String.prototype.trim`: its white space and line ends.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    })
}

/// A relay's address of JSON, trimmed, when it parses.
fn address_of(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    parse_address(s).map(|_| js_trim(s).to_string())
}

/// One field of a saved or written config, when it is one of the field's kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineField {
    Relay(Option<String>),
    RelayId(Option<String>),
    Eos(bool),
    Lan(Option<u16>),
    Direct(bool),
}

/// Field `k` of a saved or written config: its value when it is one of the field's kind (an address that parses, a
/// port), else None.
pub fn online_field(k: &str, v: &Value) -> Option<OnlineField> {
    match k {
        "relay" if v.is_null() => Some(OnlineField::Relay(None)),
        "relay" => address_of(v).map(|a| OnlineField::Relay(Some(a))),
        "relayId" if v.is_null() => Some(OnlineField::RelayId(None)),
        "relayId" => v.as_str().filter(|s| is_key_id(s)).map(|s| OnlineField::RelayId(Some(s.to_string()))),
        "lan" if v.is_null() => Some(OnlineField::Lan(None)),
        "lan" => lan_port_ok(v).map(|p| OnlineField::Lan(Some(p))),
        "eos" => v.as_bool().map(OnlineField::Eos),
        "direct" => v.as_bool().map(OnlineField::Direct),
        _ => None,
    }
}

/// A field's default as JSON, for the log.
fn default_text(k: &str) -> String {
    let d = ONLINE_DEFAULTS;
    let v = match k {
        "relay" => serde_json::to_value(&d.relay),
        "relayId" => serde_json::to_value(&d.relay_id),
        "eos" => serde_json::to_value(d.eos),
        "lan" => serde_json::to_value(d.lan),
        _ => serde_json::to_value(d.direct),
    };
    v.map(|v| v.to_string()).unwrap_or_default()
}

/// online.json read whole: a missing or unreadable file is ONLINE_DEFAULTS, a missing or bad field that field's
/// default; each one `log`ged (it is read once a run).
pub fn read_online_config(file: &Path, log: &mut dyn FnMut(String)) -> OnlineConfig {
    let raw = match std::fs::read(file)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(e) => {
            log(format!("online config: {} not read ({e}): the defaults are used", file.display()));
            return ONLINE_DEFAULTS;
        }
    };
    let empty = Map::new();
    let o = raw.as_object().unwrap_or(&empty);
    let mut out = ONLINE_DEFAULTS;
    for k in ONLINE_KEYS {
        let f = o.get(k).and_then(|v| online_field(k, v));
        match f {
            Some(f) => out.set(f),
            None => {
                let what = match o.get(k) {
                    Some(v) => format!("is not one ({v})"),
                    None => "is missing".to_string(),
                };
                log(format!("online config: {k} {what} in {}: {} is used", file.display(), default_text(k)));
            }
        }
    }
    out
}

impl OnlineConfig {
    fn set(&mut self, f: OnlineField) {
        match f {
            OnlineField::Relay(v) => self.relay = v,
            OnlineField::RelayId(v) => self.relay_id = v,
            OnlineField::Eos(v) => self.eos = v,
            OnlineField::Lan(v) => self.lan = v,
            OnlineField::Direct(v) => self.direct = v,
        }
    }
}

/// Settings.online: the fields of the online config the player set, each over online.json's
/// (a field left out, None, follows the file), and the relays the player said yes to joining a code of. `relay` and
/// `lan` are Some(None) when the player turned them off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OnlineSetting {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eos: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan: Option<Option<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct: Option<bool>,
    pub known_relays: Vec<KnownRelay>,
}

impl OnlineSetting {
    fn set(&mut self, f: OnlineField) {
        match f {
            OnlineField::Relay(v) => self.relay = Some(v),
            // (the release's relay's id is online.json's alone; a relay the player said yes to keeps its own in
            // known_relays)
            OnlineField::RelayId(_) => {}
            OnlineField::Eos(v) => self.eos = Some(v),
            OnlineField::Lan(v) => self.lan = Some(v),
            OnlineField::Direct(v) => self.direct = Some(v),
        }
    }
}

impl OnlineSetting {
    /// The relay the player said yes to at `address` (the same relay: `same_address`).
    pub fn known_relay(&self, address: &str) -> Option<&KnownRelay> {
        self.known_relays.iter().find(|r| same_address(&r.address, address))
    }

    /// A relay said yes to, or the id one proved, kept: an address known already takes a new id (an id of None leaves
    /// the one kept), a new one goes last (the oldest dropped past KNOWN_RELAYS_MAX). The release's own relay
    /// (`file`'s address with its relayId) is never kept: its id is online.json's. True when the setting changed.
    pub fn know_relay(&mut self, file: &OnlineConfig, k: KnownRelay) -> bool {
        if file.relay_id.is_some() && file.relay.as_deref().is_some_and(|f| same_address(f, &k.address)) {
            return false;
        }
        if let Some(r) = self.known_relays.iter_mut().find(|r| same_address(&r.address, &k.address)) {
            if k.id.is_none() || r.id == k.id {
                return false;
            }
            r.id = k.id;
            return true;
        }
        self.known_relays.push(k);
        let n = self.known_relays.len();
        self.known_relays.drain(..n.saturating_sub(KNOWN_RELAYS_MAX));
        true
    }
}

/// Two relay addresses name the same relay: the same host, its case aside, and the same port (`parse_address`, the
/// port RELAY_PORT when none is written). Not an address: never the same.
/// An IP literal compares as its address (`[0:0::1]` is `[::1]`), a name with one trailing dot dropped.
pub fn same_address(a: &str, b: &str) -> bool {
    let (Some(x), Some(y)) = (parse_address(a), parse_address(b)) else { return false };
    let host = |h: &str| h.strip_suffix('.').unwrap_or(h).to_owned();
    let (hx, hy) = (host(&x.host), host(&y.host));
    if x.port != y.port {
        return false;
    }
    match (hx.parse::<std::net::IpAddr>(), hy.parse::<std::net::IpAddr>()) {
        (Ok(p), Ok(q)) => p == q,
        _ => hx.eq_ignore_ascii_case(&hy),
    }
}

/// The known relays to write, merged with the settings file's on disk (`disk`, which another writer, the launcher or
/// the game, may have changed since): for an address this writer changed since `base` (what it read or last wrote),
/// its own entry; for any other, the disk's; a relay only one side knows is kept (the disk's order, then this
/// writer's new ones, the oldest dropped past KNOWN_RELAYS_MAX). Both writers changing one address: the one that
/// writes last wins.
pub fn merge_known_relays(mine: &[KnownRelay], base: &[KnownRelay], disk: &[KnownRelay]) -> Vec<KnownRelay> {
    let find = |l: &[KnownRelay], a: &str| l.iter().find(|r| same_address(&r.address, a)).cloned();
    let changed = |k: &KnownRelay| find(base, &k.address).as_ref() != Some(k);
    let mut out: Vec<KnownRelay> = Vec::new();
    for k in disk
        .iter()
        .map(|d| find(mine, &d.address).filter(changed).unwrap_or_else(|| d.clone()))
        .chain(mine.iter().cloned())
    {
        if !out.iter().any(|r| same_address(&r.address, &k.address)) {
            out.push(k);
        }
    }
    let n = out.len();
    out.drain(..n.saturating_sub(KNOWN_RELAYS_MAX));
    out
}

/// The config the program runs on: online.json's, each field the player set over it. The relay's id is the file's
/// while the relay in use is the file's (the player's own address for it included), else None: a relay the player
/// chose is checked against the id kept with it in `known_relays` (glue's pin), not here.
pub fn effective_online(file: &OnlineConfig, player: &OnlineSetting) -> OnlineConfig {
    let relay = player.relay.clone().unwrap_or_else(|| file.relay.clone());
    let release = matches!((&relay, &file.relay), (Some(a), Some(b)) if same_address(a, b));
    OnlineConfig {
        relay,
        relay_id: if release { file.relay_id.clone() } else { None },
        eos: player.eos.unwrap_or(file.eos),
        lan: player.lan.unwrap_or(file.lan),
        direct: player.direct.unwrap_or(file.direct),
    }
}

/// A saved relay the player said yes to: an address (saved before the ids were kept: id None) or { address, id }.
fn known_relay_of(v: &Value) -> Option<KnownRelay> {
    if let Some(a) = address_of(v) {
        return Some(KnownRelay { address: a, id: None });
    }
    let address = v.get("address").and_then(address_of)?;
    let id = v.get("id").and_then(Value::as_str).filter(|s| is_key_id(s)).map(str::to_string);
    Some(KnownRelay { address, id })
}

/// Settings.online from a saved or patched value, field by field (a bad field left out: it follows online.json). An
/// old saved setting, one backend chosen ({ kind: 'epic' | 'relay' | 'direct' }), becomes the player's own: a relay's
/// address the relay field (and a relay said yes to), Direct's port the LAN field, Epic nothing; a field equal to
/// `file`'s (online.json's) is left out, so it follows the file.
pub fn online_setting_of(v: &Value, file: &OnlineConfig) -> OnlineSetting {
    let empty = Map::new();
    let o = v.as_object().unwrap_or(&empty);
    if let Some(kind) = o.get("kind") {
        let mut out = OnlineSetting::default();
        match kind.as_str() {
            Some("relay") => {
                if let Some(a) = o.get("address").and_then(address_of) {
                    if file.relay.as_deref() != Some(a.as_str()) {
                        out.relay = Some(Some(a.clone()));
                    }
                    out.known_relays.push(KnownRelay { address: a, id: None });
                }
            }
            Some("direct") => {
                if let Some(p) = o.get("port").and_then(lan_port_ok).filter(|p| file.lan != Some(*p)) {
                    out.lan = Some(Some(p));
                }
            }
            _ => {}
        }
        return out;
    }
    let mut out = OnlineSetting::default();
    for k in ONLINE_KEYS {
        if let Some(f) = o.get(k).and_then(|v| online_field(k, v)) {
            out.set(f);
        }
    }
    // (one entry an address: a later one's id replaces an earlier's, in the earlier's place)
    if let Some(list) = o.get("known_relays").and_then(Value::as_array) {
        for k in list.iter().filter_map(known_relay_of) {
            match out.known_relays.iter_mut().find(|r| same_address(&r.address, &k.address)) {
                Some(r) => {
                    if k.id.is_some() {
                        r.id = k.id;
                    }
                }
                None => out.known_relays.push(k),
            }
        }
    }
    let n = out.known_relays.len();
    out.known_relays.drain(..n.saturating_sub(KNOWN_RELAYS_MAX));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn known(a: &str) -> KnownRelay {
        KnownRelay { address: a.into(), id: None }
    }

    #[test]
    fn fields_take_their_kind() {
        assert_eq!(online_field("relay", &json!(" a.b:7777 ")), Some(OnlineField::Relay(Some("a.b:7777".into()))));
        assert_eq!(online_field("relay", &json!(null)), Some(OnlineField::Relay(None)));
        assert_eq!(online_field("relay", &json!("a b")), None);
        assert_eq!(online_field("lan", &json!(7777.0)), Some(OnlineField::Lan(Some(7777))));
        assert_eq!(online_field("lan", &json!(LAN_DISCOVERY_PORT)), None);
        assert_eq!(online_field("lan", &json!(0)), None);
        assert_eq!(online_field("lan", &json!(7.5)), None);
        assert_eq!(online_field("eos", &json!(1)), None);
        assert_eq!(online_field("direct", &json!(false)), Some(OnlineField::Direct(false)));
    }

    #[test]
    fn config_file_falls_back_field_by_field() {
        let dir = std::env::temp_dir().join(format!("mvs-online-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("online.json");
        std::fs::write(&file, r#"{ "relay": "relay.example:7777", "eos": "yes", "lan": null }"#).unwrap();
        let mut lines = Vec::new();
        let c = read_online_config(&file, &mut |l| lines.push(l));
        assert_eq!(c, OnlineConfig { relay: Some("relay.example:7777".into()), ..ONLINE_DEFAULTS });
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("online config: relayId is missing"));
        assert!(lines[1].starts_with("online config: eos is not one (\"yes\")"));
        assert!(lines[2].starts_with("online config: direct is missing"));
        let missing = read_online_config(&dir.join("none.json"), &mut |l| lines.push(l));
        assert_eq!(missing, ONLINE_DEFAULTS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn setting_reads_and_overrides() {
        let s = online_setting_of(
            &json!({ "relay": null, "lan": 9000, "eos": "x", "known_relays": ["a:1", " a:1", "bad addr", "b"] }),
            &ONLINE_DEFAULTS,
        );
        assert_eq!(s.relay, Some(None));
        assert_eq!(s.lan, Some(Some(9000)));
        assert_eq!(s.eos, None);
        assert_eq!(s.known_relays, vec![known("a:1"), known("b")]);
        let file = OnlineConfig { relay: Some("r:1".into()), ..ONLINE_DEFAULTS };
        let e = effective_online(&file, &s);
        assert_eq!(e, OnlineConfig { relay: None, relay_id: None, eos: true, lan: Some(9000), direct: false });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v,
            json!({ "relay": null, "lan": 9000, "known_relays": [{ "address": "a:1" }, { "address": "b" }] })
        );
        assert_eq!(online_setting_of(&v, &ONLINE_DEFAULTS), s);
        let many: Vec<String> = (1..=40).map(|i| format!("h{i}:1")).collect();
        let s = online_setting_of(&json!({ "known_relays": many }), &ONLINE_DEFAULTS);
        assert_eq!(s.known_relays.len(), KNOWN_RELAYS_MAX);
        assert_eq!(s.known_relays[0], known("h9:1"));
    }

    const ID_A: &str = "r0123456789abcdef0123456789abcdef";
    const ID_B: &str = "rfedcba9876543210fedcba9876543210";

    fn with_id(a: &str, id: &str) -> KnownRelay {
        KnownRelay { address: a.into(), id: Some(id.into()) }
    }

    #[test]
    fn known_relays_read_one_per_address() {
        // strings and {address,id}; one per address (host case and the default port aside); a later id replaces
        let s = online_setting_of(
            &json!({ "known_relays": [
                "a.example:1",
                { "address": "b.example", "id": ID_A },
                { "address": "A.EXAMPLE:1", "id": ID_A },
                { "address": "b.example:7777", "id": ID_B },
                { "address": "b.example", "id": "not an id" },
                { "address": "c d", "id": ID_A },
                { "id": ID_A },
            ] }),
            &ONLINE_DEFAULTS,
        );
        assert_eq!(s.known_relays, vec![with_id("a.example:1", ID_A), with_id("b.example", ID_B)]);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["known_relays"][1], json!({ "address": "b.example", "id": ID_B }));
        assert_eq!(online_setting_of(&v, &ONLINE_DEFAULTS), s);
    }

    #[test]
    fn the_release_id_follows_the_release_relay() {
        let file = OnlineConfig { relay: Some("relay.example".into()), relay_id: Some(ID_A.into()), ..ONLINE_DEFAULTS };
        let id = |player: Option<Option<&str>>| {
            let s = OnlineSetting { relay: player.map(|p| p.map(str::to_string)), ..OnlineSetting::default() };
            effective_online(&file, &s).relay_id
        };
        assert_eq!(id(None).as_deref(), Some(ID_A));
        assert_eq!(id(Some(Some("RELAY.example:7777"))).as_deref(), Some(ID_A));
        assert_eq!(id(Some(Some("relay.example:7778"))), None);
        assert_eq!(id(Some(Some("other.example"))), None);
        assert_eq!(id(Some(None)), None);
        assert!(same_address("[::1]:9", "[::1]:9") && !same_address("x y", "x y"));
    }

    #[test]
    fn a_relay_known_takes_its_new_id() {
        let file = OnlineConfig { relay: Some("relay.example".into()), relay_id: Some(ID_A.into()), ..ONLINE_DEFAULTS };
        let mut s = OnlineSetting::default();
        assert!(s.know_relay(&file, known("x.example:1")));
        assert!(!s.know_relay(&file, known("X.example:1")));
        assert!(s.know_relay(&file, with_id("x.example:1", ID_A)));
        assert!(!s.know_relay(&file, with_id("x.example:1", ID_A)));
        // an id of None leaves the one kept
        assert!(!s.know_relay(&file, known("x.example:1")));
        assert!(s.know_relay(&file, with_id("X.EXAMPLE:1", ID_B)));
        assert_eq!(s.known_relays, vec![with_id("x.example:1", ID_B)]);
        assert_eq!(s.known_relay("x.example:1").and_then(|k| k.id.as_deref()), Some(ID_B));
        // the release's relay is online.json's
        assert!(!s.know_relay(&file, with_id("relay.example:7777", ID_B)));
        let unpinned = OnlineConfig { relay_id: None, ..file.clone() };
        assert!(s.know_relay(&unpinned, with_id("relay.example", ID_B)));
        for i in 0..KNOWN_RELAYS_MAX {
            s.know_relay(&file, known(&format!("h{i}:1")));
        }
        assert_eq!(s.known_relays.len(), KNOWN_RELAYS_MAX);
        assert_eq!(s.known_relays[0], known("h0:1"));
    }

    #[test]
    fn ip_hosts_compare_as_addresses() {
        assert!(same_address("[0:0::1]:9", "[::1]:9"));
        assert!(same_address("relay.example.:9", "RELAY.example:9"));
        assert!(same_address("127.0.0.1", "127.0.0.1:7777"));
        assert!(!same_address("127.0.0.1", "127.0.0.2"));
        assert!(!same_address("[::1]:9", "[::1]:10"));
    }

    /// Two writers of one settings file: each one's own change of an address wins, the other's are taken from the
    /// disk, and a relay either one added is kept.
    #[test]
    fn known_relays_merge_with_the_disk() {
        let base = vec![with_id("a:1", ID_A), with_id("b:1", ID_A)];
        // the other writer: trusted b's new key and added c
        let disk = vec![with_id("a:1", ID_A), with_id("b:1", ID_B), with_id("c:1", ID_A)];
        // this one: a's new key, d added, b untouched
        let mine = vec![with_id("a:1", ID_B), with_id("b:1", ID_A), with_id("d:1", ID_A)];
        let merged = merge_known_relays(&mine, &base, &disk);
        assert_eq!(
            merged,
            vec![with_id("a:1", ID_B), with_id("b:1", ID_B), with_id("c:1", ID_A), with_id("d:1", ID_A)]
        );
        // nothing changed here: the disk's whole
        assert_eq!(merge_known_relays(&base, &base, &disk), disk);
        // through a file (Settings::save_changes)
        let dir = std::env::temp_dir().join(format!("mvs-known-merge-{}", std::process::id()));
        let mut on_disk = crate::Settings::default();
        on_disk.online.known_relays.clone_from(&disk);
        on_disk.save(&dir).unwrap();
        let mut s = crate::Settings::default();
        s.online.known_relays = mine;
        let mut b = crate::Settings::default();
        b.online.known_relays = base;
        s.save_changes(&mut b, &dir).unwrap();
        assert_eq!(s.online.known_relays, merged);
        assert_eq!(b.online.known_relays, merged);
        assert_eq!(crate::Settings::load(&dir).online.known_relays, merged);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_kind_setting_becomes_fields() {
        let file = OnlineConfig { relay: Some("r:1".into()), ..ONLINE_DEFAULTS };
        let s = online_setting_of(&json!({ "kind": "relay", "address": "r:1" }), &file);
        assert_eq!(s, OnlineSetting { known_relays: vec![known("r:1")], ..Default::default() });
        let s = online_setting_of(&json!({ "kind": "relay", "address": "s:2" }), &file);
        assert_eq!(s.relay, Some(Some("s:2".into())));
        let s = online_setting_of(&json!({ "kind": "direct", "port": 7777 }), &file);
        assert_eq!(s.lan, Some(Some(7777)));
        let s = online_setting_of(&json!({ "kind": "direct", "port": 8000 }), &file);
        assert_eq!(s.lan, Some(Some(8000)));
        assert_eq!(online_setting_of(&json!({ "kind": "epic" }), &file), OnlineSetting::default());
    }
}
