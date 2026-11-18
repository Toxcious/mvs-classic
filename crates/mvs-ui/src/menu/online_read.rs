//! What the screens read of `OnlineView` (the getters of web/src/net/online.ts and net/party.ts): the party's size
//! and leader, a listed player by id, the online list's empty line, and the change test that stands for the TS's
//! onChanged listeners.

use mvs_protocol::online::PARTY_MAX;

use super::online::{BrowseNote, OnlinePlayer, OnlineView};
use super::online_text::listed_text;
use super::widgets::social::SocialPlayerData;
use mvs_net::service::api::PartyView;

impl OnlineView {
    /// The party has two members: the build's GetIsPartyFull (net/party.ts full).
    pub fn party_full(&self) -> bool {
        self.party.as_ref().is_some_and(|p| p.members.len() >= PARTY_MAX)
    }

    /// This player is the party's leader (party.leading).
    pub fn party_leading(&self) -> bool {
        self.party.as_ref().is_some_and(|p| p.leader == p.me)
    }

    /// A player of the online list by id (online.listedPlayer).
    pub fn listed_player(&self, id: &str) -> Option<&OnlinePlayer> {
        self.listed.iter().find(|p| p.id == id)
    }

    /// Why the online list is empty when it could not be asked, in words ('' when it was; online.listedText).
    pub fn listed_why_text(&self) -> String {
        self.listed_why.as_ref().map(listed_text).unwrap_or_default()
    }

    /// The page's session is the queue's (online.queued).
    pub fn queued(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.queued)
    }
}

impl From<&OnlinePlayer> for SocialPlayerData {
    fn from(p: &OnlinePlayer) -> SocialPlayerData {
        SocialPlayerData {
            id: p.id.clone(),
            name: p.name.clone(),
            busy: p.busy,
            present: p.present,
            icon: p.icon.clone(),
            status: p.status,
        }
    }
}

/// What a list of players shows of the view: the data a panel redraws on when any of it changes (the TS's
/// online.onChanged and party.onChange).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineSeen {
    connected: bool,
    players: Vec<OnlinePlayer>,
    listed: Vec<OnlinePlayer>,
    why: Option<BrowseNote>,
    party: Option<PartyView>,
    queued: bool,
}

impl OnlineSeen {
    /// The view's lists as they are now.
    pub fn of(v: &OnlineView) -> OnlineSeen {
        OnlineSeen {
            connected: v.connected,
            players: v.players.clone(),
            listed: v.listed.clone(),
            why: v.listed_why.clone(),
            party: v.party.clone(),
            queued: v.queued(),
        }
    }

    /// Whether the view's lists are as this was taken (no clone).
    pub fn same(&self, v: &OnlineView) -> bool {
        self.connected == v.connected
            && self.players == v.players
            && self.listed == v.listed
            && self.why == v.listed_why
            && self.party == v.party
            && self.queued == v.queued()
    }
}
