//! (the port's own) The custom lobby's connection line, a status label under the page's title: the state in capitals
//! ("ONLINE", "LAN", "OFFLINE") and what the lobby is connected through ("via Relay", "via Epic" or the host's
//! address); a local lobby says why in one to three words, and what to do is the line's tip. The round trip is the
//! top-right ping line's, not the line's. The build's lobby has no such line (WBP_CustomGame shows the room code alone).

use super::custom_text as t;
use super::online::{LocalReason, OfflineWhy, SessionLink};
use super::online_line::reason;

/// The line's state: its first word and that word's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineState {
    /// On the relay or Epic: others can join over the internet.
    Online,
    /// On a direct link: others can join on the local network (or by the host's address).
    LocalNetwork,
    /// Local: this PC's players alone.
    Offline,
}

/// What the line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineText {
    pub state: LineState,
    /// The state, in capitals.
    pub head: &'static str,
    /// What the lobby is connected through, or why a local one is local.
    pub detail: String,
    /// What to do for a local lobby to be online, the line's tip (None: an online lobby).
    pub how: Option<String>,
}

/// The line of a lobby on `link`.
pub fn line(link: &SessionLink) -> LineText {
    let (state, head, detail) = match link {
        SessionLink::Relay => (LineState::Online, t::LOBBY_HEAD_ONLINE, t::LOBBY_ON_RELAY.to_owned()),
        SessionLink::Epic => (LineState::Online, t::LOBBY_HEAD_ONLINE, t::LOBBY_ON_EPIC.to_owned()),
        SessionLink::Lan { at } => (LineState::LocalNetwork, t::LOBBY_HEAD_LAN, at.clone()),
        SessionLink::Local(why) => {
            let (reason, how) = local_reason(why);
            return LineText {
                state: LineState::Offline,
                head: t::LOBBY_HEAD_OFFLINE,
                detail: reason.to_owned(),
                how: Some(how),
            };
        }
    };
    LineText { state, head, detail, how: None }
}

/// Why a lobby is local, and what to do.
fn local_reason(why: &LocalReason) -> (&'static str, String) {
    let (reason, how) = match why {
        LocalReason::RelayVersion => (t::LOBBY_LOCAL_RELAY_VERSION, t::LOBBY_LOCAL_RELAY_VERSION_DO),
        LocalReason::CannotHost => (t::LOBBY_LOCAL_CANNOT_HOST, t::LOBBY_LOCAL_CANNOT_HOST_DO),
        LocalReason::MadeOffline => (t::LOBBY_LOCAL_MADE_OFFLINE, t::LOBBY_LOCAL_MADE_OFFLINE_DO),
        // (online play's own reasons: the menus' line's words)
        LocalReason::Offline(off) => {
            let how = match off {
                OfflineWhy::TurnedOff => t::LOBBY_LOCAL_TURNED_OFF_DO,
                OfflineWhy::SigningIn => t::LOBBY_LOCAL_SIGNING_IN_DO,
                OfflineWhy::NotReached => t::LOBBY_LOCAL_NOT_REACHED_DO,
                OfflineWhy::Checking => t::LOBBY_LOCAL_CHECKING_DO,
                OfflineWhy::Update { version } => {
                    return (reason(off), t::LOBBY_LOCAL_UPDATE_DO.replace("{version}", version));
                }
                OfflineWhy::Import => t::LOBBY_LOCAL_IMPORT_DO,
                OfflineWhy::SelfCheck => t::LOBBY_LOCAL_SELF_CHECK_DO,
                OfflineWhy::Other => t::LOBBY_LOCAL_OTHER_DO,
            };
            (reason(off), how)
        }
    };
    (reason, how.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_online_state_has_its_words() {
        let l = line(&SessionLink::Relay);
        assert_eq!((l.state, l.head, l.detail.as_str()), (LineState::Online, "ONLINE", "via Relay"));
        assert!(l.how.is_none());
        let l = line(&SessionLink::Epic);
        assert_eq!((l.head, l.detail.as_str()), ("ONLINE", "via Epic"));
        let l = line(&SessionLink::Lan { at: "192.168.1.5:7777".into() });
        assert_eq!((l.state, l.head, l.detail.as_str()), (LineState::LocalNetwork, "LAN", "192.168.1.5:7777"));
    }

    #[test]
    fn a_local_lobby_says_why() {
        let l = line(&SessionLink::Local(LocalReason::RelayVersion));
        assert_eq!((l.state, l.head, l.detail.as_str()), (LineState::Offline, "OFFLINE", "Server version differs"));
        assert_eq!(l.how.as_deref(), Some(t::LOBBY_LOCAL_RELAY_VERSION_DO));
    }

    #[test]
    fn no_line_has_a_separator() {
        let mut links = vec![SessionLink::Relay, SessionLink::Epic];
        for why in [LocalReason::RelayVersion, LocalReason::CannotHost, LocalReason::MadeOffline] {
            links.push(SessionLink::Local(why));
        }
        for link in links {
            let l = line(&link);
            let all = [l.head.to_owned(), l.detail, l.how.unwrap_or_default()];
            assert!(all.iter().all(|s| !s.contains(['\u{b7}', '|', ':'])), "{all:?}");
        }
    }

    #[test]
    fn each_local_reason_has_its_short_words() {
        let r = |why: LocalReason| {
            let l = line(&SessionLink::Local(why));
            (l.detail, l.how.unwrap())
        };
        assert_eq!(r(LocalReason::CannotHost).0, t::LOBBY_LOCAL_CANNOT_HOST);
        assert_eq!(r(LocalReason::MadeOffline).0, t::LOBBY_LOCAL_MADE_OFFLINE);
        assert_eq!(r(LocalReason::Offline(OfflineWhy::TurnedOff)).0, t::WHY_TURNED_OFF);
        assert_eq!(r(LocalReason::Offline(OfflineWhy::SigningIn)).0, t::WHY_SIGNING_IN);
        assert_eq!(r(LocalReason::Offline(OfflineWhy::NotReached)).0, t::WHY_NOT_REACHED);
        let update = r(LocalReason::Offline(OfflineWhy::Update { version: "0.2.0".into() }));
        assert_eq!(update, ("Update needed".into(), "Update to 0.2.0 in the launcher".into()));
        for why in [OfflineWhy::Checking, OfflineWhy::Import, OfflineWhy::SelfCheck, OfflineWhy::Other] {
            let (reason, how) = r(LocalReason::Offline(why));
            assert!(reason.split(' ').count() <= 3 && how.len() <= 50, "short: {reason} / {how}");
        }
    }
}
