//! Why a backend's call failed, as an id the menus word (`mvs_ui::menu::online_text`): the backends answer a call
//! with their own text (an EOS result's name, the relay's or the direct host's reason), which `NetFail::of` sorts into
//! the reasons the player is told. The text stays the log's and the wire's: `text` gives it back as it came.

use serde::{Deserialize, Serialize};

/// What a call to the direct host after its end is answered with (the closed host).
pub(crate) const HOST_CLOSED: &str = "the direct host has closed";
/// What a call the direct host cannot write is answered with.
pub(crate) const UNWRITABLE: &str = "a request that cannot be written";
/// What a call to a closed relay connection is answered with.
pub(crate) const RELAY_CLOSED: &str = "the relay connection is closed";
/// What the relay's core refuses a player with when it holds its limit (mvs_relay core.rs join).
const RELAY_FULL: &str = "relay full";
/// EOS's result for a call refused for its rate.
const LIMIT_EXCEEDED: &str = "EOS_LimitExceeded";
/// EOS's answer to a lobby made whose details cannot be read.
pub(crate) const LOBBY_UNREAD: &str = "the lobby could not be read";

/// A backend's reason, sorted. The player reads the menus' words of it; `Code` shows an EOS result's name as it is (a
/// name, not a sentence), `Other` none of its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum NetFail {
    /// The direct host has ended (the LAN host's own session closed under it).
    HostClosed,
    /// The direct host could not write the call.
    Unwritable,
    /// The relay connection is closed.
    RelayClosed,
    /// The relay holds as many players as it takes.
    RelayFull,
    /// The backend's rate refused it (EOS_LimitExceeded).
    Limited,
    /// The lobby made could not be read back.
    LobbyUnread,
    /// A backend's result name (EOS_Lobby_TooManyPlayers, EOS_Disabled): shown as it is.
    Code { code: String },
    /// Any other reason: its text for the log only.
    Other { log: String },
}

impl NetFail {
    /// The backend's text sorted into its reason.
    pub fn of(text: &str) -> NetFail {
        match text {
            HOST_CLOSED => NetFail::HostClosed,
            UNWRITABLE => NetFail::Unwritable,
            RELAY_CLOSED => NetFail::RelayClosed,
            RELAY_FULL => NetFail::RelayFull,
            LOBBY_UNREAD => NetFail::LobbyUnread,
            LIMIT_EXCEEDED => NetFail::Limited,
            t if is_code(t) => NetFail::Code { code: t.to_owned() },
            t => NetFail::Other { log: t.to_owned() },
        }
    }

    /// The text the backend gave (the log's and the wire's).
    pub fn text(&self) -> String {
        match self {
            NetFail::HostClosed => HOST_CLOSED.to_owned(),
            NetFail::Unwritable => UNWRITABLE.to_owned(),
            NetFail::RelayClosed => RELAY_CLOSED.to_owned(),
            NetFail::RelayFull => RELAY_FULL.to_owned(),
            NetFail::Limited => LIMIT_EXCEEDED.to_owned(),
            NetFail::LobbyUnread => LOBBY_UNREAD.to_owned(),
            NetFail::Code { code } => code.clone(),
            NetFail::Other { log } => log.clone(),
        }
    }
}

/// A result's name: EOS_ and letters, digits and underscores.
fn is_code(s: &str) -> bool {
    s.starts_with("EOS_") && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl std::fmt::Display for NetFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

impl From<String> for NetFail {
    fn from(t: String) -> NetFail {
        NetFail::of(&t)
    }
}

impl From<NetFail> for String {
    fn from(f: NetFail) -> String {
        f.text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backends_text_sorts_into_its_reason_and_back() {
        assert_eq!(NetFail::of("the direct host has closed"), NetFail::HostClosed);
        assert_eq!(NetFail::of("relay full"), NetFail::RelayFull);
        assert_eq!(NetFail::of("EOS_Lobby_TooManyPlayers"), NetFail::Code { code: "EOS_Lobby_TooManyPlayers".into() });
        assert_eq!(NetFail::of("EOS relay: EOS_Disabled"), NetFail::Other { log: "EOS relay: EOS_Disabled".into() });
        assert_eq!(NetFail::of("EOS_LimitExceeded"), NetFail::Limited);
        assert_eq!(NetFail::of("some words"), NetFail::Other { log: "some words".into() });
        for t in ["the direct host has closed", "relay full", "EOS_Disabled", "some words"] {
            let j = serde_json::to_string(&NetFail::of(t)).unwrap_or_default();
            assert_eq!(j, serde_json::to_string(t).unwrap_or_default());
        }
    }
}
