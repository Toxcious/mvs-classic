//! The words of the codes the online layers send (the net service's reasons, the notices of the game): the player
//! reads them in the menus; the wire carries codes only. Every word is a const of `custom_text`; this module only maps
//! a code to its const and fills its blanks.

use mvs_net::backend::BackendKind;
use mvs_net::codes::{CodeMakeWhy, CodeWhy};
use mvs_net::fail::NetFail;
use mvs_net::lan::direct::{DirectJoinWhy, HostStartWhy};
use mvs_net::lan::direct_code::DirectWhy;
use mvs_net::service::api::{
    AskWhat, BackendOff, BackendText, BrowseWhy, ClosedWhy, EpicMissing, JoinWhy, OffWhy, PartyJoinWhy, StatusWhy,
};

use super::custom_text as t;
use super::online::{BrowseNote, OnlineNotice};

/// A backend's name inside a reason.
pub fn backend_name(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::Relay => t::BACKEND_NAME_RELAY,
        BackendKind::Epic => t::BACKEND_NAME_EPIC,
        BackendKind::Lan => t::BACKEND_NAME_LAN,
    }
}

/// A retry delay as it is worded: whole seconds from a second on, else milliseconds.
fn retry_in(in_ms: u32) -> String {
    if in_ms >= 1000 { format!("{} s", (f64::from(in_ms) / 1000.0).round()) } else { format!("{in_ms} ms") }
}

/// Why a backend is off. `kind` names the backend for the reasons that name it.
pub fn off_why_text(kind: BackendKind, why: &OffWhy) -> String {
    match why {
        OffWhy::Off => t::OFF_WHY_OFF.to_string(),
        OffWhy::NotOn => t::OFF_WHY_NOT_ON.replace("{backend}", backend_name(kind)),
        OffWhy::SigningIn => t::OFF_WHY_SIGNING_IN.to_string(),
        OffWhy::Connecting { address } => t::OFF_WHY_CONNECTING.replace("{address}", address),
        OffWhy::SignInChanged => t::OFF_WHY_SIGN_IN_CHANGED.replace("{backend}", backend_name(kind)),
        OffWhy::LanFailed => t::OFF_WHY_LAN_FAILED.to_string(),
        OffWhy::Missing { why: EpicMissing::NotInstalled } => t::OFF_WHY_NOT_INSTALLED.to_string(),
        OffWhy::Missing { why: EpicMissing::NotSetUp } => t::OFF_WHY_NOT_SET_UP.to_string(),
        OffWhy::Backend { text } => text.clone(),
        OffWhy::Retrying { text, in_ms } => {
            t::OFF_WHY_RETRYING.replace("{text}", text).replace("{in}", &retry_in(*in_ms))
        }
        OffWhy::SettingsChanged => t::OFF_WHY_SETTINGS_CHANGED.to_string(),
        OffWhy::TurnedOff => t::OFF_WHY_TURNED_OFF.to_string(),
        OffWhy::LanPortChanged => t::OFF_WHY_LAN_PORT.to_string(),
        OffWhy::LanSettingChanged => t::OFF_WHY_LAN_SETTING.to_string(),
        OffWhy::RelayKey { release, in_ms } => {
            let text = if *release { t::OFF_WHY_RELAY_KEY_RELEASE } else { t::OFF_WHY_RELAY_KEY_KNOWN };
            text.replace("{in}", &retry_in(*in_ms))
        }
    }
}

/// The status's reason while offline: each backend off as '<Name>: <why>' joined by '; ' ('signing in' before any
/// has one).
pub fn backends_off_text(backends: &[BackendOff]) -> String {
    if backends.is_empty() {
        return t::STATUS_SIGNING_IN.to_string();
    }
    let list: Vec<String> =
        backends.iter().map(|b| format!("{}: {}", backend_name(b.kind), off_why_text(b.kind, &b.why))).collect();
    list.join("; ")
}

/// The status's why ('' online).
pub fn status_why_text(why: &StatusWhy) -> String {
    match why {
        StatusWhy::Online => String::new(),
        StatusWhy::SigningIn => t::STATUS_SIGNING_IN.to_string(),
        StatusWhy::Off { backends } => backends_off_text(backends),
    }
}

/// A browse's failed backends: each '<the search on Name failed: why>', LAN's '<LAN: why>',
/// joined by '; '.
fn failed_text(backends: &[BackendText]) -> String {
    let list: Vec<String> = backends
        .iter()
        .map(|b| match b.kind {
            BackendKind::Lan => format!("{}: {}", backend_name(b.kind), b.text),
            k => t::SEARCH_FAILED.replace("{name}", backend_name(k)).replace("{why}", &b.text),
        })
        .collect();
    list.join("; ")
}

/// A browse's or the online list's reason as the net service gave it.
pub fn browse_why_text(why: &BrowseWhy) -> String {
    match why {
        BrowseWhy::All => String::new(),
        BrowseWhy::Offline { why } => format!("{}{}", t::ONLINE_OFF, status_why_text(why)),
        BrowseWhy::ListOff { why } => t::LIST_RELAY_OFF.replace("{why}", &off_why_text(BackendKind::Relay, why)),
        BrowseWhy::NoList => t::LIST_NO_LIST.to_string(),
        BrowseWhy::ListFailed { text } => format!("{}{}", t::LIST_FAILED, text),
        BrowseWhy::Failed { backends } => failed_text(backends),
    }
}

/// The online list's empty line: '' when it was asked.
pub fn listed_text(note: &BrowseNote) -> String {
    match note {
        BrowseNote::Net(w) => browse_why_text(w),
        BrowseNote::NoApp => t::ONLINE_LIST_NO_APP.to_string(),
        BrowseNote::NoAnswer => t::ONLINE_LIST_NO_ANSWER.to_string(),
    }
}

/// The custom game browser's line for a browse that found none or not all.
pub fn browse_text(note: &BrowseNote) -> String {
    match note {
        BrowseNote::Net(w) => browse_why_text(w),
        BrowseNote::NoApp => t::BROWSE_NO_APP.to_string(),
        BrowseNote::NoAnswer => t::BROWSE_NO_ANSWER.to_string(),
    }
}

/// Why a text is not a lobby code.
pub fn code_why_text(why: CodeWhy) -> String {
    match why {
        CodeWhy::Empty => t::CODE_EMPTY.to_string(),
        CodeWhy::UnknownKind => t::CODE_UNKNOWN_KIND.to_string(),
        CodeWhy::Mistyped => t::CODE_MISTYPED.to_string(),
        CodeWhy::Version(v) => t::CODE_VERSION.replace("{v}", &v.to_string()),
        CodeWhy::Spelled => t::CODE_SPELLED.to_string(),
        CodeWhy::DirectMistyped => t::DIRECT_CODE_MISTYPED.to_string(),
        CodeWhy::DirectVersion(v) => t::DIRECT_CODE_VERSION.replace("{v}", &v.to_string()),
        CodeWhy::DirectOutside => t::DIRECT_CODE_OUTSIDE.to_string(),
    }
}

/// Why a LAN lobby's direct host did not start, without the make's lead.
pub fn host_start_why_text(why: &HostStartWhy) -> String {
    match why {
        HostStartWhy::KeyRead { file, text } => t::HOST_START_KEY_READ.replace("{file}", file).replace("{why}", text),
        HostStartWhy::KeyWrite { file, text } => t::HOST_START_KEY_WRITE.replace("{file}", file).replace("{why}", text),
        HostStartWhy::NoNetwork => t::HOST_START_NO_NETWORK.to_string(),
        HostStartWhy::CannotListen { text: None } => t::HOST_START_CANNOT_LISTEN.to_string(),
        HostStartWhy::CannotListen { text: Some(text) } => t::HOST_START_CANNOT_LISTEN_WHY.replace("{why}", text),
        HostStartWhy::PortsInUse { first, last } => {
            t::HOST_START_PORTS_IN_USE.replace("{first}", &first.to_string()).replace("{last}", &last.to_string())
        }
        HostStartWhy::ThreadStopped => t::HOST_START_THREAD_STOPPED.to_string(),
        HostStartWhy::OwnRefused { why } => t::HOST_START_OWN_REFUSED.replace("{why}", &net_fail_text(why)),
    }
}

/// A backend's reason for a failed call, without the lead.
pub fn net_fail_text(why: &NetFail) -> String {
    match why {
        NetFail::HostClosed => t::FAIL_HOST_CLOSED.to_string(),
        NetFail::Unwritable => t::FAIL_UNWRITABLE.to_string(),
        NetFail::RelayClosed => t::FAIL_RELAY_CLOSED.to_string(),
        NetFail::RelayFull => t::FAIL_RELAY_FULL.to_string(),
        NetFail::Limited => t::LIMITED.to_string(),
        NetFail::LobbyUnread => t::FAIL_LOBBY_UNREAD.to_string(),
        NetFail::Code { code } => t::FAIL_CODE.replace("{code}", code),
        NetFail::Other { .. } => t::FAIL_OTHER.to_string(),
    }
}

/// Why a lobby's code cannot be written, without the make's lead.
pub fn code_make_why_text(why: &CodeMakeWhy) -> String {
    match why {
        CodeMakeWhy::DirectKind => t::CODE_MAKE_DIRECT_KIND.to_string(),
        CodeMakeWhy::EpicId { lobby } => t::CODE_MAKE_EPIC_ID.replace("{lobby}", lobby),
        CodeMakeWhy::RelayId { lobby } => t::CODE_MAKE_RELAY_ID.replace("{lobby}", lobby),
        CodeMakeWhy::NoDefaultRelay => t::CODE_MAKE_NO_DEFAULT_RELAY.to_string(),
        CodeMakeWhy::RelayAddress { relay } => t::CODE_MAKE_RELAY_ADDRESS.replace("{relay}", relay),
        CodeMakeWhy::DirectAddress { address } => t::CODE_MAKE_DIRECT_ADDRESS.replace("{address}", address),
        CodeMakeWhy::DirectNoAddress => t::CODE_MAKE_DIRECT_NO_ADDRESS.to_string(),
        CodeMakeWhy::DirectPort { port } => t::CODE_MAKE_DIRECT_PORT.replace("{port}", &port.to_string()),
        CodeMakeWhy::DirectFingerprint => t::CODE_MAKE_DIRECT_FINGERPRINT.to_string(),
        CodeMakeWhy::DirectLobby => t::CODE_MAKE_DIRECT_LOBBY.to_string(),
    }
}

/// Why a join by a direct code failed.
pub fn direct_join_why_text(why: &DirectJoinWhy) -> String {
    match why {
        DirectJoinWhy::Code(DirectWhy::Mistyped) => t::DIRECT_CODE_MISTYPED.to_string(),
        DirectJoinWhy::Code(DirectWhy::Version(v)) => t::DIRECT_CODE_VERSION.replace("{v}", &v.to_string()),
        DirectJoinWhy::Code(DirectWhy::Outside) => t::DIRECT_CODE_OUTSIDE.to_string(),
        DirectJoinWhy::Key(e) => e.clone(),
        DirectJoinWhy::Cancelled => t::DIRECT_CANCELLED.to_string(),
        DirectJoinWhy::HostRefused(w) => t::DIRECT_HOST_REFUSED.replace("{why}", w),
        DirectJoinWhy::WrongHost => t::DIRECT_WRONG_HOST.to_string(),
        DirectJoinWhy::Unreachable(each) => {
            let each: Vec<String> = each.iter().map(|(a, w)| format!("{a}: {w}")).collect();
            t::DIRECT_UNREACHABLE.replace("{each}", &each.join("; "))
        }
    }
}

/// Why a join by a code failed, without the 'the lobby could not be joined: ' lead.
pub fn join_why_text(why: &JoinWhy) -> String {
    match why {
        JoinWhy::BadCode { why } => code_why_text(*why),
        JoinWhy::Backend { text } => text.clone(),
        JoinWhy::Direct { why } => direct_join_why_text(why),
        JoinWhy::Gone => t::JOIN_GONE.to_string(),
        JoinWhy::Full => t::JOIN_FULL.to_string(),
        JoinWhy::NoDirect => t::JOIN_NO_DIRECT.to_string(),
        JoinWhy::EpicOff { why } => t::JOIN_EPIC_OFF.replace("{why}", &off_why_text(BackendKind::Epic, why)),
        JoinWhy::NoDefaultRelay => t::JOIN_NO_DEFAULT_RELAY.to_string(),
        JoinWhy::ShortOtherRelay => t::JOIN_SHORT_OTHER_RELAY.to_string(),
        JoinWhy::RelayOff { address, why } => {
            t::JOIN_RELAY_OFF.replace("{address}", address).replace("{why}", &off_why_text(BackendKind::Relay, why))
        }
        JoinWhy::NoRelayLink => t::JOIN_NO_RELAY_LINK.to_string(),
        JoinWhy::RelayNotTaken { address } => t::JOIN_RELAY_NOT_TAKEN.replace("{address}", address),
        JoinWhy::RelayRefused { address, text } => {
            format!("{}{}", t::JOIN_RELAY_REFUSED.replace("{address}", address), text)
        }
    }
}

/// Why a party join failed, without the 'the party could not be joined: ' lead.
pub fn party_join_why_text(why: &PartyJoinWhy) -> String {
    match why {
        PartyJoinWhy::NoInvite => t::PARTY_JOIN_NO_INVITE.to_string(),
        PartyJoinWhy::NotRelay => t::PARTY_JOIN_NOT_RELAY.to_string(),
        PartyJoinWhy::OtherRelay { address } => {
            t::PARTY_JOIN_OTHER_RELAY.replace("{address}", address.as_deref().unwrap_or(t::PARTY_JOIN_THE_DEFAULT))
        }
        PartyJoinWhy::RelayOff { why } => {
            t::PARTY_JOIN_RELAY_OFF.replace("{why}", &off_why_text(BackendKind::Relay, why))
        }
        PartyJoinWhy::NotInviter => t::PARTY_JOIN_NOT_INVITER.to_string(),
        PartyJoinWhy::Backend { text } => text.clone(),
        PartyJoinWhy::BadCode { why } => code_why_text(*why),
    }
}

/// Why a session (or a party) is over, in a sentence.
pub fn closed_why_text(why: &ClosedWhy) -> String {
    let lead = |lead: &str, rest: &str| format!("{lead}{rest}");
    match why {
        ClosedWhy::Left => t::CLOSED_LEFT.to_string(),
        ClosedWhy::Replaced => t::CLOSED_REPLACED.to_string(),
        ClosedWhy::Stopped => t::CLOSED_STOPPED.to_string(),
        ClosedWhy::NoSession => t::CLOSED_NO_SESSION.to_string(),
        ClosedWhy::Offline { why } => lead(t::ONLINE_OFF, &status_why_text(why)),
        ClosedWhy::NoQueue => lead(t::ONLINE_OFF, t::NO_QUEUE),
        ClosedWhy::NotRelayed { text } => lead(t::CLOSED_NOT_RELAYED, text),
        ClosedWhy::BackendOff { kind, why } => {
            t::CLOSED_BACKEND_OFF.replace("{backend}", backend_name(*kind)).replace("{why}", &off_why_text(*kind, why))
        }
        ClosedWhy::RelayLost { text } => lead(t::CLOSED_RELAY_LOST, text),
        ClosedWhy::Limited => t::LIMITED.to_string(),
        ClosedWhy::MakeFailed { why } => lead(t::CLOSED_MAKE_FAILED, &net_fail_text(why)),
        ClosedWhy::MakeHostFailed { why } => lead(t::CLOSED_MAKE_FAILED, &host_start_why_text(why)),
        ClosedWhy::MakeCodeFailed { why } => lead(t::CLOSED_MAKE_FAILED, &code_make_why_text(why)),
        ClosedWhy::JoinFailed { why } => lead(t::CLOSED_JOIN_FAILED, &join_why_text(why)),
        ClosedWhy::NoHost => t::CLOSED_NO_HOST.to_string(),
        ClosedWhy::HostSilent => t::CLOSED_HOST_SILENT.to_string(),
        ClosedWhy::BadPacket => t::CLOSED_BAD_PACKET.to_string(),
        ClosedWhy::HostClosed => t::CLOSED_HOST_CLOSED.to_string(),
        ClosedWhy::HostLeft => t::CLOSED_HOST_LEFT.to_string(),
        ClosedWhy::Kicked => t::CLOSED_KICKED.to_string(),
        ClosedWhy::LobbyClosed => t::CLOSED_LOBBY_CLOSED.to_string(),
        ClosedWhy::LobbyGone { text } => lead(t::CLOSED_LOBBY_GONE, text),
        ClosedWhy::HostLost => t::CLOSED_HOST_LOST.to_string(),
        ClosedWhy::HostLinkLost { text } => lead(t::CLOSED_HOST_LINK_LOST, text),
        ClosedWhy::DirectStopped { text } => lead(t::CLOSED_DIRECT_STOPPED, text),
        ClosedWhy::SendFailed => t::CLOSED_SEND_FAILED.to_string(),
        // (the host core's words: HostWhy::log_name)
        ClosedWhy::Host { why } => why.log_name().to_string(),
        ClosedWhy::BadGuestPacket => t::CLOSED_BAD_GUEST_PACKET.to_string(),
        ClosedWhy::PartyEnded => t::CLOSED_PARTY_ENDED.to_string(),
        ClosedWhy::PartyEndedBy { why } => lead(t::CLOSED_PARTY_ENDED_BY, &closed_why_text(why)),
        ClosedWhy::PartyLobbyClosed { text } => lead(t::CLOSED_PARTY_LOBBY_CLOSED, text),
        ClosedWhy::QueueMoved => t::CLOSED_QUEUE_MOVED.to_string(),
        ClosedWhy::QueueUnmatched => t::CLOSED_QUEUE_UNMATCHED.to_string(),
        ClosedWhy::PartyMakeFailed { why } => lead(t::CLOSED_PARTY_MAKE_FAILED, &net_fail_text(why)),
        ClosedWhy::PartyMakeCodeFailed { why } => lead(t::CLOSED_PARTY_MAKE_FAILED, &code_make_why_text(why)),
        ClosedWhy::NoLeader => t::CLOSED_NO_LEADER.to_string(),
        ClosedWhy::PartyJoinFailed { why } => lead(t::CLOSED_PARTY_JOIN_FAILED, &party_join_why_text(why)),
        ClosedWhy::LeaderSilent => t::CLOSED_LEADER_SILENT.to_string(),
        ClosedWhy::LeaderLeft => t::CLOSED_LEADER_LEFT.to_string(),
        ClosedWhy::LeaderLost => t::CLOSED_LEADER_LOST.to_string(),
        ClosedWhy::Other { text } => text.clone(),
        ClosedWhy::MakeNoRelay => lead(t::CLOSED_MAKE_FAILED, t::CODE_ON_NO_RELAY),
        ClosedWhy::PartyMakeNoRelay => lead(t::CLOSED_PARTY_MAKE_FAILED, t::CODE_ON_NO_RELAY),
        ClosedWhy::BadAnswer => t::CLOSED_BAD_ANSWER.to_string(),
    }
}

/// The session's question: its body and the words of its yes and no.
pub fn ask_text(what: &AskWhat) -> (String, &'static str, &'static str) {
    match what {
        AskWhat::NewRelay { address } => (t::ASK_NEW_RELAY.replace("{address}", address), t::ASK_YES, t::ASK_NO),
        AskWhat::RelayKeyChanged { address } => {
            (t::ASK_RELAY_KEY_CHANGED.replace("{address}", address), t::ASK_YES, t::ASK_NO)
        }
    }
}

/// A note of the game.
pub fn notice_text(n: &OnlineNotice) -> String {
    match n {
        OnlineNotice::Host(text) => text.clone(),
        OnlineNotice::ProtocolDiffers => t::PROTOCOL_DIFFERS.to_string(),
        OnlineNotice::InviteNotSent { .. } => t::INVITE_REFUSED.to_string(),
        OnlineNotice::Desync { .. } => t::DESYNC_NOTE.to_string(),
        // (the main menu shows it with the build's own title, main_menu.rs party_disbanded)
        OnlineNotice::PartyDisbanded => String::new(),
    }
}

/// The note a pre-match screen kicks back to the main menu with.
pub fn kick_text(n: &super::online::KickNote) -> String {
    use super::online::KickNote;
    match n {
        KickNote::Error => t::PM_ERROR.to_string(),
        KickNote::Left => t::PM_LEFT.to_string(),
        KickNote::LeftNamed(name) => t::PM_LEFT_NAMED.replace("{username}", name),
        KickNote::CompatMissing => t::COMPAT_MISSING.to_string(),
        KickNote::PartyRefused => t::PARTY_REFUSED.to_string(),
        KickNote::PartySearchEnded => t::PARTY_SEARCH_ENDED.to_string(),
        KickNote::PartyLeft(name) => t::PARTY_LEFT.replace("{username}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menus' words of each code are those mvs-net keeps as the log's (`Display`).
    #[test]
    fn the_made_lobbys_whys_read_as_the_ts() {
        let hosts = [
            HostStartWhy::KeyRead { file: "k.pem".into(), text: "denied".into() },
            HostStartWhy::KeyWrite { file: "k.pem".into(), text: "full".into() },
            HostStartWhy::NoNetwork,
            HostStartWhy::CannotListen { text: None },
            HostStartWhy::CannotListen { text: Some("denied".into()) },
            HostStartWhy::PortsInUse { first: 7777, last: 7786 },
            HostStartWhy::ThreadStopped,
            HostStartWhy::OwnRefused { why: NetFail::RelayFull },
        ];
        for w in &hosts {
            assert_eq!(host_start_why_text(w), w.to_string());
        }
        let codes = [
            CodeMakeWhy::DirectKind,
            CodeMakeWhy::EpicId { lobby: "x".into() },
            CodeMakeWhy::RelayId { lobby: "l1".into() },
            CodeMakeWhy::NoDefaultRelay,
            CodeMakeWhy::RelayAddress { relay: "a b".into() },
            CodeMakeWhy::DirectAddress { address: "8.8.8.8".into() },
            CodeMakeWhy::DirectNoAddress,
            CodeMakeWhy::DirectPort { port: 0 },
            CodeMakeWhy::DirectFingerprint,
            CodeMakeWhy::DirectLobby,
        ];
        for w in &codes {
            assert_eq!(code_make_why_text(w), w.to_string());
        }
        assert_eq!(
            closed_why_text(&ClosedWhy::MakeHostFailed { why: HostStartWhy::PortsInUse { first: 1, last: 10 } }),
            "the lobby could not be made: ports 1 to 10 are all in use"
        );
        assert_eq!(
            closed_why_text(&ClosedWhy::PartyMakeCodeFailed { why: CodeMakeWhy::NoDefaultRelay }),
            "the party could not be made: a lobby code of the default relay needs one: this app has none"
        );
    }

    /// A short code's refusals (the port's own): another relay than the release's, and a mistyped one.
    #[test]
    fn a_short_codes_join_whys() {
        assert_eq!(join_why_text(&JoinWhy::ShortOtherRelay), t::JOIN_SHORT_OTHER_RELAY);
        let mistyped = JoinWhy::BadCode { why: CodeWhy::Mistyped };
        assert_eq!(join_why_text(&mistyped), t::CODE_MISTYPED);
        let read = mvs_net::codes::decode_lobby_code("k7m2p0", None).expect_err("mistyped");
        assert_eq!(code_why_text(read), t::CODE_MISTYPED);
    }

    /// A relay that proved another key: the release's, one said yes to; the question of a code's relay whose key
    /// changed.
    #[test]
    fn a_relays_other_key_is_worded() {
        let release = off_why_text(BackendKind::Relay, &OffWhy::RelayKey { release: true, in_ms: 4000 });
        assert!(
            release.starts_with("the relay did not prove") && release.ends_with("(trying again in 4 s)"),
            "{release}"
        );
        let known = off_why_text(BackendKind::Relay, &OffWhy::RelayKey { release: false, in_ms: 500 });
        assert!(known.contains("when you last played") && known.ends_with("(trying again in 500 ms)"), "{known}");
        let (q, yes, no) = ask_text(&AskWhat::RelayKeyChanged { address: "relay.example:9000".into() });
        assert!(q.starts_with("The relay at relay.example:9000 proved another key") && q.ends_with('?'), "{q}");
        assert_eq!((yes, no), (t::ASK_YES, t::ASK_NO));
    }
}
