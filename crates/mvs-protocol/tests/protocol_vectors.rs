//! The protocol's pure functions against the oracle's golden vectors (`protocol.json` of
//! the oracle's protocol vectors): setup, loadout, queue rules, chat, the lobby listing and the lobby as it travels.
//! A test that finds no vectors says so and passes.
//!
//! The vectors tag what JSON loses (`$f64` for NaN / infinity / -0, `$undef`, `$utf16` for a string with a lone
//! surrogate).
//!
//! The lobby's style is the port's own (`LobbyStyle`: Versus and Free For All, LISTING_V 2), where the oracle's lobby
//! has ETeamStyle's 1v1, 2v2 and Free For All (LISTING_V 1): its 2v2 is compared as Versus (the same team rules: two a
//! team) and its Free For All as Free For All, each by its new number; a lobby script's stretch in the oracle's 1v1
//! (one a team, which Versus has not) is run but not compared, and the compare resumes at the oracle's next 2v2 or
//! Free For All from the oracle's state there (`LobbyRules::load`); a listing is read as its v 2 form.
//!
//! A Rust string holds no lone surrogate (nor does a message that reaches it: serde_json refuses one), so
//! a lone surrogate reads as U+E000 (a private use character: no mark, space or
//! format character, and no grapheme break before a mark, as the lone half is not); a NaN or infinity reads as null (a
//! number that is no integer reads as one that is not a number, in every function here).

use mvs_protocol::chat::{self, ChatGate};
use mvs_protocol::loadout::{self, NetLoadout};
use mvs_protocol::lobby_listing::{self, LobbyListing};
use mvs_protocol::lobby_rules::{LobbyRules, LobbyStyle, MutatorSet, OptionKey, OptionValue};
use mvs_protocol::online::{QueueMode, QueuePlayer};
use mvs_protocol::queue_rules as qr;
use mvs_protocol::setup::{self, InputPrefsWire, MatchPlayerWire};
use mvs_protocol::vectors;
use serde_json::{Map, Value, json};

/// A vector value as the Rust functions take it.
fn untag(v: &Value) -> Value {
    match v {
        Value::Object(o) if o.len() == 1 => {
            if let Some(h) = o.get("$f64").and_then(Value::as_str) {
                let f = f64::from_bits(u64::from_str_radix(h, 16).expect("f64 bits"));
                return if f.is_finite() { json!(f) } else { Value::Null };
            }
            if o.contains_key("$undef") {
                return Value::Null;
            }
            if let Some(u) = o.get("$utf16").and_then(Value::as_array) {
                let units = u.iter().map(|x| u16::try_from(x.as_u64().expect("unit")).expect("u16"));
                return Value::String(char::decode_utf16(units).map(|c| c.unwrap_or('\u{E000}')).collect());
            }
            Value::Object(o.iter().map(|(k, v)| (k.clone(), untag(v))).collect())
        }
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, v) in o {
                if v.as_object().is_some_and(|x| x.len() == 1 && x.contains_key("$undef")) {
                    continue;
                }
                m.insert(k.clone(), untag(v));
            }
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(untag).collect()),
        v => v.clone(),
    }
}

/// Numbers as f64 (-0 as 0), so 250 and 250.0 and -0 compare equal.
fn norm(v: Value) -> Value {
    match v {
        Value::Number(n) => {
            let f = n.as_f64().expect("a number");
            json!(if f == 0.0 { 0.0 } else { f })
        }
        Value::Array(a) => Value::Array(a.into_iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, norm(v))).collect()),
        v => v,
    }
}

fn same(got: &impl serde::Serialize, want: &Value, what: &str, at: usize) {
    let g = norm(serde_json::to_value(got).expect("serialisable"));
    let w = norm(untag(want));
    assert_eq!(g, w, "{what} {at}: in {}", serde_json::to_string(want).unwrap_or_default());
}

/// Whether a `$utf16` string holds a lone high half and, after only control and format units (which cleanChat takes
/// out), a lone low half: the oracle then has one pair where a Rust string cannot have had two lone halves.
fn rejoins(v: &Value) -> bool {
    let Some(u) = v.get("$utf16").and_then(Value::as_array) else { return false };
    let units: Vec<u32> = u.iter().map(|x| u32::try_from(x.as_u64().unwrap()).unwrap()).collect();
    let removed = |c: u32| {
        c <= 0x1f
            || (0x7f..=0x9f).contains(&c)
            || c == 0xad
            || (0x200b..=0x200f).contains(&c)
            || (0x202a..=0x202e).contains(&c)
            || (0x2060..=0x2064).contains(&c)
            || (0x2066..=0x206f).contains(&c)
    };
    (0..units.len()).any(|i| {
        let high = (0xd800..0xdc00).contains(&units[i]);
        let lone = high && !units.get(i + 1).is_some_and(|n| (0xdc00..0xe000).contains(n));
        lone && units[i + 1..].iter().find(|c| !removed(**c)).is_some_and(|c| (0xdc00..0xe000).contains(c))
    })
}

/// Numbers that are whole as integers (a serde integer field takes no -0.0).
fn whole(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(whole).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, whole(v))).collect()),
        v => v,
    }
}

fn field<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or_else(|| panic!("vector field {k}"))
}

fn mode(v: &Value) -> QueueMode {
    QueueMode::of(v.as_str().expect("a mode")).expect("a known mode")
}

#[test]
fn read_input_prefs() {
    let Some(v) = vectors::read("protocol.json") else { return };
    for (i, c) in field(&v, "readInputPrefs").as_array().unwrap().iter().enumerate() {
        same(&setup::read_input_prefs(&untag(&c["in"])), &c["out"], "readInputPrefs", i);
    }
}

#[test]
fn loadouts() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let l = field(&v, "loadout");
    assert_eq!(field(&l["consts"], "TAUNT_SLOTS"), &json!(loadout::TAUNT_SLOTS));
    assert_eq!(field(&l["consts"], "BADGE_SLOTS"), &json!(loadout::BADGE_SLOTS));
    for (i, c) in l["readLoadout"].as_array().unwrap().iter().enumerate() {
        same(&loadout::read_loadout(&untag(&c["in"])), &c["out"], "readLoadout", i);
    }
    for (i, c) in l["cosmeticsOf"].as_array().unwrap().iter().enumerate() {
        // (a count the oracle cleans to -0 reads back as the whole number 0)
        let input: NetLoadout = serde_json::from_value(whole(untag(&c["in"]))).expect("a loadout");
        same(&loadout::cosmetics_of(&input), &c["out"], "cosmeticsOf", i);
    }
    let mut n = 0;
    for (i, c) in l["withCosmetics"].as_array().unwrap().iter().enumerate() {
        let a = untag(&c["in"]);
        let a = a.as_array().expect("arguments");
        let p: MatchPlayerWire = serde_json::from_value(a[0].clone()).expect("a player");
        let taunts: Option<Vec<String>> = serde_json::from_value(a[1].clone()).expect("taunts");
        let text = |x: &Value| x.as_str().map(str::to_owned);
        // (the input is a typed InputPrefsWire here: a value the type cannot hold is a case the typed caller cannot
        // make)
        let Ok(input) = serde_json::from_value::<Option<InputPrefsWire>>(a[5].clone()) else { continue };
        let got = loadout::with_cosmetics(
            &p,
            taunts.as_deref(),
            text(&a[2]).as_deref(),
            text(&a[3]).as_deref(),
            text(&a[4]).as_deref(),
            input.as_ref(),
        );
        same(&got, &c["out"], "withCosmetics", i);
        n += 1;
    }
    eprintln!("withCosmetics: {n} compared");
}

#[test]
fn queue_rules() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let q = field(&v, "queueRules");
    let k = &q["consts"];
    for (name, got) in [
        ("PARTY_WAIT_MS", qr::PARTY_WAIT_MS),
        ("BOT_AFTER_S", qr::BOT_AFTER_S),
        ("BOT_AFTER_MIN_S", qr::BOT_AFTER_MIN_S),
        ("PREMATCH_MS", qr::PREMATCH_MS),
        ("PM_PICK_MS", qr::PM_PICK_MS),
        ("PM_PERKS_MS", qr::PM_PERKS_MS),
        ("PM_BOT_LOCK_MS", qr::PM_BOT_LOCK_MS),
        ("PM_BOT_RELOCK_MS", qr::PM_BOT_RELOCK_MS),
        ("SET_WINS", f64::from(qr::SET_WINS)),
        ("SET_GAMES", f64::from(qr::SET_GAMES)),
        ("VOTE_MS", qr::VOTE_MS),
        ("VOTE_PAGE_S", qr::VOTE_PAGE_S),
        ("VOTE_BOT_MS", qr::VOTE_BOT_MS),
        ("REMATCH_MS", qr::REMATCH_MS),
        ("RESULT_MS", qr::RESULT_MS),
        ("QUEUE_BOT_DIFFICULTY", f64::from(qr::QUEUE_BOT_DIFFICULTY)),
        ("QUEUE_RINGOUTS", f64::from(qr::QUEUE_RINGOUTS)),
        ("MAX_PERKS", qr::MAX_PERKS as f64),
    ] {
        assert_eq!(k[name].as_f64(), Some(got), "const {name}");
    }
    assert_eq!(norm(k["VOTE_BOT_GAP_MS"].clone()), norm(json!(qr::VOTE_BOT_GAP_MS)));
    for m in qr::QUEUE_MODES {
        assert_eq!(k["QUEUE_SIZE"][m.id()].as_u64(), Some(qr::queue_size(m) as u64), "size {m:?}");
        assert_eq!(k["QUEUE_TEAM_STYLE"][m.id()].as_u64(), Some(u64::from(qr::queue_team_style(m))), "style {m:?}");
    }
    for (i, c) in q["perMode"].as_array().unwrap().iter().enumerate() {
        let m = mode(&c["mode"]);
        assert_eq!(c["ticketMax"].as_u64(), Some(qr::ticket_max(m) as u64), "ticketMax {i}");
        assert_eq!(c["setWins"].as_u64(), Some(u64::from(qr::set_wins(m))), "setWins {i}");
        assert_eq!(c["setGames"].as_u64(), Some(u64::from(qr::set_games(m))), "setGames {i}");
        assert_eq!(c["queueTeams"].as_u64(), Some(qr::queue_teams(m) as u64), "queueTeams {i}");
        let teams: Vec<usize> = (0..9).map(|s| qr::queue_team(m, s)).collect();
        assert_eq!(c["teams"], json!(teams), "queueTeam {i}");
    }
    for (i, c) in q["botAfterS"].as_array().unwrap().iter().enumerate() {
        same(&qr::bot_after_s(&untag(&c["in"])), &c["out"], "botAfterS", i);
    }
    let mut skipped_slots = 0;
    for (i, c) in q["queueSlots"].as_array().unwrap().iter().enumerate() {
        // (a ticket that is no count of players, -1, 1.5, NaN, is one the typed caller cannot make: 0 refuses as they
        // do)
        let given = untag(&c["tickets"]);
        // (1.5 players in a ticket is a count the typed caller cannot make: the oracle takes it as one, a half slot)
        // (and a NaN is a count that passes the oracle's tests, where a Rust count is a number)
        let nan = c["tickets"].as_array().unwrap().iter().any(|t| {
            t.get("$f64").and_then(Value::as_str).is_some_and(|h| h.starts_with("7ff8") || h.starts_with("fff8"))
        });
        if nan || given.as_array().unwrap().iter().any(|t| t.as_f64().is_some_and(|f| f.fract() != 0.0)) {
            skipped_slots += 1;
            continue;
        }
        let tickets: Vec<u32> =
            given.as_array().unwrap().iter().map(|t| t.as_u64().map_or(0, |n| u32::try_from(n).unwrap())).collect();
        same(&qr::queue_slots(mode(&c["mode"]), &tickets), &c["out"], "queueSlots", i);
    }
    eprintln!("queueSlots: {skipped_slots} cases with a fractional ticket skipped");
    for (i, c) in q["pickBots"].as_array().unwrap().iter().enumerate() {
        let strings = |x: &Value| -> Vec<String> {
            x.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect()
        };
        let r: Vec<u64> = c["rand"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
        let (mut at, mut seen) = (0, Vec::new());
        let mut rand = |m: u32| {
            seen.push(m);
            at += 1;
            u32::try_from(r[(at - 1) % r.len()] % u64::from(m)).unwrap()
        };
        let got =
            qr::pick_bots(&strings(&c["pool"]), &strings(&c["humans"]), c["n"].as_u64().unwrap() as usize, &mut rand);
        assert_eq!(json!(got), c["out"], "pickBots {i}");
        assert_eq!(json!(seen), c["randCalls"], "pickBots rand calls {i}");
    }
    let mut skipped = 0;
    for (i, c) in q["queueSetup"].as_array().unwrap().iter().enumerate() {
        let Some(duration) = c["duration"].as_u64().and_then(|d| u32::try_from(d).ok()) else {
            skipped += 1;
            continue;
        };
        let players: Vec<QueuePlayer> = c["players"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| QueuePlayer {
                bot: p["bot"].as_bool().unwrap(),
                character: p["character"].as_str().unwrap_or_default().to_owned(),
                ..QueuePlayer::default()
            })
            .collect();
        same(&qr::queue_setup(mode(&c["mode"]), &players, duration), &c["out"], "queueSetup", i);
    }
    eprintln!("queueSetup: {skipped} cases with a duration that is no u32 skipped");
    for (i, c) in q["cleanPerks"].as_array().unwrap().iter().enumerate() {
        same(&qr::clean_perks(&untag(&c["in"])), &c["out"], "cleanPerks", i);
    }
}

#[test]
fn chat_rules() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let c = field(&v, "chat");
    let k = &c["consts"];
    assert_eq!(k["CHAT_LIMIT"].as_u64(), Some(chat::CHAT_LIMIT as u64));
    assert_eq!(k["CHAT_BURST"].as_f64(), Some(chat::CHAT_BURST));
    assert_eq!(k["CHAT_PER_SEC"].as_f64(), Some(chat::CHAT_PER_SEC));
    assert_eq!(k["CHAT_EMOJIS"].as_u64(), Some(chat::CHAT_EMOJIS as u64));
    assert_eq!(k["CHAT_BUBBLE_MS"].as_f64(), Some(chat::CHAT_BUBBLE_MS));
    assert_eq!(k["CHAT_FADE_MS"].as_f64(), Some(chat::CHAT_FADE_MS));
    assert_eq!(k["CHAT_BUBBLES"].as_u64(), Some(chat::CHAT_BUBBLES as u64));
    let mut bad = Vec::new();
    let mut rejoined = 0;
    for (i, x) in c["cleanChat"].as_array().unwrap().iter().enumerate() {
        let got = chat::clean_chat(&untag(&x["in"]));
        let want = untag(&x["out"]);
        if json!(got) != want {
            if rejoins(&x["in"]) {
                rejoined += 1;
                continue;
            }
            bad.push((i, x["in"].to_string(), got, want));
        }
    }
    for b in bad.iter().take(6) {
        eprintln!(
            "cleanChat {}: in {}\n  got  {:?}\n  want {}",
            b.0,
            b.1.chars().take(300).collect::<String>(),
            b.2,
            b.3
        );
    }
    eprintln!(
        "cleanChat: {rejoined} cases differ only where the oracle joins two lone halves the cleaning leaves side by side"
    );
    assert!(bad.is_empty(), "cleanChat: {} cases differ", bad.len());
    for (i, x) in c["chatLength"].as_array().unwrap().iter().enumerate() {
        let s = untag(&x["in"]);
        // (a value that is no string is one the typed caller cannot pass)
        let Some(s) = s.as_str() else { continue };
        assert_eq!(x["out"].as_u64(), Some(chat::chat_length(s) as u64), "chatLength {i}");
    }
    for (i, x) in c["chatGate"].as_array().unwrap().iter().enumerate() {
        let num = |v: &Value| untag(v).as_f64().unwrap_or(f64::NAN);
        let mut gate = ChatGate::new(x["times"].as_f64().unwrap_or(1.0), num(&x["t0"]));
        let got: Vec<bool> = x["at"].as_array().unwrap().iter().map(|t| gate.allow(num(t))).collect();
        assert_eq!(json!(got), x["taken"], "chatGate {i}");
    }
}

/// The oracle's lobby style (ETeamStyle) as the port's: its 2v2 is Versus, its Free For All Free For All; None for
/// its 1v1 (no style of the port's) or a number that is no style.
fn style_of(n: f64) -> Option<LobbyStyle> {
    match n {
        1.0 => Some(LobbyStyle::Versus),
        2.0 => Some(LobbyStyle::FreeForAll),
        _ => None,
    }
}

/// A listing's text with the number after its own key `key` (after `{` or `,`, not in a string) as `f` gives it;
/// the text as it was when `f` gives None or the key or its number is not there.
fn move_number(t: &str, key: &str, f: impl Fn(f64) -> Option<String>) -> String {
    let pat = format!("\"{key}\":");
    let mut from = 0;
    while let Some(at) = t[from..].find(&pat).map(|i| i + from) {
        let start = at + pat.len();
        if matches!(t[..at].chars().last(), Some('{' | ',')) {
            let end = t[start..].find([',', '}']).map_or(t.len(), |i| i + start);
            return match t[start..end].trim().parse::<f64>().ok().and_then(f) {
                Some(n) => format!("{}{n}{}", &t[..start], &t[end..]),
                None => t.to_owned(),
            };
        }
        from = start;
    }
    t.to_owned()
}

/// The oracle's listing text in the port's form: its v 1 as 2 (and a 2 as 1, still another shape), its style by
/// `style_of` (the oracle's 1v1 as Versus: a listing's reader takes either the same).
fn listing_v2(t: &str) -> String {
    let t = move_number(t, "v", |v| match v {
        1.0 => Some("2".into()),
        2.0 => Some("1".into()),
        _ => None,
    });
    move_number(&t, "style", |n| {
        let style = if n == 0.0 { Some(LobbyStyle::Versus) } else { style_of(n) };
        style.map(|s| u8::from(s).to_string())
    })
}

/// The oracle's read listing (an object or null) in the port's form.
fn listing_out_v2(v: &Value) -> Value {
    let mut v = untag(v);
    if let Some(o) = v.as_object_mut() {
        o.insert("v".into(), json!(lobby_listing::LISTING_V));
        let n = o["style"].as_f64().expect("a read listing's style");
        let style = if n == 0.0 { LobbyStyle::Versus } else { style_of(n).expect("a style") };
        o.insert("style".into(), json!(u8::from(style)));
    }
    v
}

#[test]
fn lobby_listing() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let l = field(&v, "lobbyListing");
    // (the oracle's listing is v 1: `listing_v2`)
    assert_eq!(l["consts"]["LISTING_V"].as_u64(), Some(1));
    assert_eq!(lobby_listing::LISTING_V, 2);
    assert_eq!(l["consts"]["LISTING_MAX_BYTES"].as_u64(), Some(lobby_listing::LISTING_MAX_BYTES as u64));
    assert_eq!(l["consts"]["LISTING_MAPS"].as_u64(), Some(lobby_listing::LISTING_MAPS as u64));
    for (i, c) in l["listingText"].as_array().unwrap().iter().enumerate() {
        let input = untag(&c["in"]).to_string();
        let input: LobbyListing = serde_json::from_str(&listing_v2(&input)).expect("a listing");
        let want = untag(&c["out"]).as_str().map(listing_v2).expect("a text");
        assert_eq!(lobby_listing::listing_text(&input), want, "listingText {i}");
    }
    let mut some = 0;
    for (i, c) in l["listingOf"].as_array().unwrap().iter().enumerate() {
        let input = untag(&c["in"]);
        let input = input.as_str().map_or(input.clone(), |t| Value::String(listing_v2(t)));
        let got = lobby_listing::listing_of(&input);
        some += usize::from(got.is_some());
        same(&got, &listing_out_v2(&c["out"]), "listingOf", i);
    }
    eprintln!("listingOf: {some} read");
}

#[test]
fn lobby_data() {
    let Some(v) = vectors::read("protocol.json") else { return };
    let (mut steps, mut skipped, mut resumed, mut sized) = (0, 0, 0, 0);
    for (s, script) in field(&v, "lobbyRules")["scripts"].as_array().unwrap().iter().enumerate() {
        let mut lobby = LobbyRules::new();
        // (in an oracle 1v1 stretch: the ops still run, nothing is compared; the stretch ends at the oracle's next
        // setTeamStyle to 2v2 or Free For All, where the oracle's state is loaded and the compare resumes)
        let mut in_one_v_one = false;
        for (i, step) in script["steps"].as_array().unwrap().iter().enumerate() {
            let op = step["op"].as_array().unwrap();
            let text = |n: usize| op[n].as_str().unwrap_or_default();
            let num = |n: usize| op[n].as_i64().unwrap();
            match text(0) {
                "addPlayer" => {
                    lobby.add_player(text(1), text(2), text(3));
                }
                "addBot" => {
                    lobby.add_bot(usize::try_from(num(1)).unwrap(), text(2), text(3), i32::try_from(num(4)).unwrap())
                }
                "setTeamStyle" => {
                    let n = op[1].as_f64().unwrap();
                    if n == 0.0 {
                        in_one_v_one = true;
                    } else {
                        lobby.set_team_style(style_of(n).expect("a style"));
                    }
                }
                "removePlayer" => {
                    lobby.remove_player(text(1));
                }
                "joinTeam" => lobby.join_team(text(1), usize::try_from(num(2)).unwrap()),
                "setReady" => lobby.set_ready(text(1), op[2].as_bool().unwrap()),
                "setMaps" => {
                    let maps: Vec<String> =
                        op[1].as_array().unwrap().iter().map(|m| m.as_str().unwrap().to_owned()).collect();
                    lobby.set_maps(&maps);
                }
                "promoteNewLeader" => {
                    lobby.promote_new_leader(text(1));
                }
                "updateSettingsForBot" => {
                    lobby.update_settings_for_bot(text(1), text(2), i32::try_from(num(3)).unwrap(), text(4));
                }
                "setIcon" => {
                    lobby.set_icon(text(1), text(2));
                }
                "setOption" => {
                    let key = match text(1) {
                        "public" => OptionKey::Public,
                        "stocks" => OptionKey::Stocks,
                        "invitedOnly" => OptionKey::InvitedOnly,
                        _ => OptionKey::Mutators,
                    };
                    // (the oracle's lobby had one mutator, 0 none: here the set of it)
                    let value = match op[2].as_bool() {
                        Some(b) => OptionValue::Bool(b),
                        None => OptionValue::Set(MutatorSet::default().with(usize::try_from(num(2)).unwrap(), true)),
                    };
                    lobby.set_option(key, value);
                }
                "set" => match text(1) {
                    "matchDuration" => lobby.match_duration = u32::try_from(num(2)).unwrap(),
                    "numRingoutsForWin" => lobby.num_ringouts_for_win = u32::try_from(num(2)).unwrap(),
                    _ => lobby.allow_hazards = op[2].as_bool().unwrap(),
                },
                other => panic!("an op the vectors hold: {other}"),
            }
            let want = lobby_data_of(&step["data"]);
            if in_one_v_one {
                if text(0) == "setTeamStyle" && style_of(op[1].as_f64().unwrap()).is_some() {
                    in_one_v_one = false;
                    lobby.load(&serde_json::from_value(want).expect("the oracle's lobby data"));
                    resumed += 1;
                }
                skipped += 1;
                continue;
            }
            // (the oracle's lobby stops at four, two a Versus team; the port's takes eight, four a team: where the
            // port's lobby is past those sizes, everything but the teams is compared and the compare goes on from
            // the oracle's teams)
            let got = norm(serde_json::to_value(lobby.data()).unwrap());
            if past_build_sizes(&got) {
                let (mut g, mut w) = (got.clone(), norm(untag(&want)));
                g.as_object_mut().unwrap().remove("teams");
                w.as_object_mut().unwrap().remove("teams");
                assert_eq!(g, w, "lobby data past the build's sizes, but for its teams, step {}", s * 100 + i);
                if got != norm(untag(&want)) {
                    lobby.load(&serde_json::from_value(want).expect("the oracle's lobby data"));
                    sized += 1;
                }
                steps += 1;
                continue;
            }
            same(&lobby.data(), &want, "lobby data", s * 100 + i);
            steps += 1;
        }
    }
    eprintln!(
        "lobby data: {steps} steps compared, {skipped} in an oracle 1v1 skipped, {resumed} resumed from the oracle's state, {sized} past the build's sizes resumed from the oracle's teams"
    );
}

/// Whether the port's lobby state is past the sizes the build's lobby takes: more than four players, or a Versus
/// team of more than two.
fn past_build_sizes(data: &Value) -> bool {
    let teams = data["teams"].as_array().unwrap();
    let sizes: Vec<usize> = teams.iter().map(|t| t.as_array().unwrap().len()).collect();
    let versus = data["teamStyle"].as_f64() == Some(f64::from(u8::from(LobbyStyle::Versus)));
    sizes.iter().sum::<usize>() > 4 || (versus && sizes.iter().any(|&n| n > 2))
}

/// An oracle lobby state in the port's form: an icon the oracle cleared by setIcon (it stays as icon:'' there; here
/// every empty icon is left out), the one mutator as its set, the style as the port's (its 1v1 as Versus).
fn lobby_data_of(data: &Value) -> Value {
    let mut want = untag(data);
    for team in want["teams"].as_array_mut().unwrap() {
        for p in team.as_array_mut().unwrap() {
            if p["icon"] == json!("") {
                p.as_object_mut().unwrap().remove("icon");
            }
        }
    }
    // (the oracle's one mutator, 0 none, is the set of it here)
    if let Some(o) = want["options"].as_object_mut()
        && let Some(n) = o.remove("mutator")
    {
        let n = usize::try_from(n.as_u64().unwrap()).unwrap();
        o.insert("mutators".into(), json!(MutatorSet::default().with(n, true).0));
    }
    let n = want["teamStyle"].as_f64().unwrap();
    want["teamStyle"] = json!(u8::from(style_of(n).unwrap_or(LobbyStyle::Versus)));
    want
}
