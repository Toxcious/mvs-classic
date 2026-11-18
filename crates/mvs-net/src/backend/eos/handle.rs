//! The EOS backend as the service holds it (the Eos object): a handle on the process's one
//! platform; every call made under its lock.

use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::IndexSet;

use super::callbacks::{on_lobby_id, on_result, on_send_invite};
use super::sdk::{self, Handle};
use super::{Left, Modified, Op, Phase, Platform, cstr, lock};
use crate::backend::{
    Answer, Ask, Backend, BackendEvent, BackendKind, Channel, Listed, P2P_MAX_PACKET, Presence, RelayMode,
};

/// A handle on the process's platform, as the service holds it.
pub(super) struct EosBackend {
    pub(super) p: Arc<Mutex<Platform>>,
    /// This player's id, as of the sign-in.
    pub(super) me: String,
}

impl EosBackend {
    fn p(&self) -> MutexGuard<'_, Platform> {
        lock(&self.p)
    }
}

impl Backend for EosBackend {
    fn me(&self) -> &str {
        &self.me
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Epic
    }

    fn create_lobby(&mut self, bucket: &str, max: u32, listed: Option<Listed>) -> Ask {
        let mut p = self.p();
        let ask = p.ask();
        if let Err(e) = p.open() {
            p.answer(ask, Err(e));
            return ask;
        }
        // A listed lobby is made invite-only and listed with its attributes in one change after, so no search finds
        // it without `at`.
        let b = cstr(bucket);
        let o = sdk::CreateLobbyOptions {
            api_version: 10,
            local_user_id: p.local,
            max_lobby_members: max,
            permission_level: sdk::LPL_INVITEONLY,
            presence_enabled: 0,
            allow_invites: 0,
            bucket_id: b.as_ptr(),
            disable_host_migration: 1,
            enable_rtc_room: 0,
            local_rtc_options: null_mut(),
            lobby_id: null(),
            enable_join_by_id: 1,
            rejoin_after_kick_requires_invite: 0,
            allowed_platform_ids: null_mut(),
            allowed_platform_ids_count: 0,
            crossplay_opt_out: 0,
            rtc_room_join_action_type: 0,
        };
        let cd = p.op(Op::Create { ask, bucket: bucket.to_owned(), listed });
        // SAFETY: the lobby interface is the platform's; the options are copied by the call.
        unsafe { (p.sdk.create_lobby)(p.lobby_api, &o, cd, on_lobby_id) };
        ask
    }

    fn search_lobbies(&mut self, bucket: &str, limit: u32, full: bool) -> Ask {
        let mut p = self.p();
        let ask = p.ask();
        if let Err(e) = p.open() {
            p.answer(ask, Err(e));
            return ask;
        }
        if limit == 0 {
            p.answer(ask, Ok(Answer::Found(Vec::new())));
            return ask;
        }
        // room for this player's own listed lobbies of the bucket, which are left out
        let own = p.lobbies.values().filter(|j| j.listed.as_deref() == Some(bucket)).count();
        let max_results = sdk::MAX_SEARCH_RESULTS.min(limit.saturating_add(u32::try_from(own).unwrap_or(u32::MAX)));
        let o = sdk::CreateLobbySearchOptions { api_version: 1, max_results };
        let mut search: Handle = null_mut();
        // SAFETY: the lobby interface is the platform's; the options and the out handle are this frame's.
        let made = unsafe { (p.sdk.create_lobby_search)(p.lobby_api, &o, &mut search) };
        if made != sdk::SUCCESS {
            let e = p.text(made);
            p.answer(ask, Err(e));
            return ask;
        }
        let b = cstr(bucket);
        let mut params =
            vec![(sdk::SEARCH_BUCKET, sdk::AttributeValue { as_utf8: b.as_ptr() }, sdk::AT_STRING, sdk::CO_EQUAL)];
        if !full {
            params.push((
                sdk::SEARCH_MIN_SLOTS,
                sdk::AttributeValue { as_int64: 1 },
                sdk::AT_INT64,
                sdk::CO_GREATERTHANOREQUAL,
            ));
        }
        for (key, value, value_type, comparison_op) in params {
            let k = cstr(key);
            let data = sdk::AttributeData { api_version: 1, key: k.as_ptr(), value, value_type };
            let so = sdk::SetParameterOptions { api_version: 1, parameter: &data, comparison_op };
            // SAFETY: the search handle was made above; the parameter and its strings live through the call.
            let r = unsafe { (p.sdk.search_set_parameter)(search, &so) };
            if r != sdk::SUCCESS {
                // SAFETY: released once.
                unsafe { (p.sdk.search_release)(search) };
                let e = p.text(r);
                p.answer(ask, Err(e));
                return ask;
            }
        }
        let fo = sdk::FindOptions { api_version: 1, local_user_id: p.local };
        let cd = p.op(Op::Search { ask, search, limit });
        // SAFETY: as above; the search is released when it is answered.
        unsafe { (p.sdk.search_find)(search, &fo, cd, on_result) };
        ask
    }

    fn set_listed(&mut self, id: &str, listed: Option<Listed>) -> Ask {
        let mut p = self.p();
        let ask = p.ask();
        if let Err(e) = p.open() {
            p.answer(ask, Err(e));
            return ask;
        }
        if !p.lobbies.get(id).is_some_and(|j| j.owned) {
            p.answer(ask, Err("not this player's lobby".to_owned()));
            return ask;
        }
        let then = Modified::SetListed { ask, id: id.to_owned(), listed: listed.is_some() };
        let r = p.modify(
            id,
            |p, m| match &listed {
                Some(l) => p.list_with(m, l),
                None => p.permission(m, sdk::LPL_INVITEONLY),
            },
            then,
        );
        if let Err((e, then)) = r {
            p.modify_failed(then, e);
        }
        ask
    }

    fn join_lobby(&mut self, id: &str) -> Ask {
        let mut p = self.p();
        let ask = p.ask();
        if let Err(e) = p.open() {
            p.answer(ask, Err(e));
            return ask;
        }
        let lobby = cstr(id);
        let o = sdk::JoinLobbyByIdOptions {
            api_version: 3,
            lobby_id: lobby.as_ptr(),
            local_user_id: p.local,
            presence_enabled: 0,
            local_rtc_options: null_mut(),
            crossplay_opt_out: 0,
            rtc_room_join_action_type: 0,
        };
        let cd = p.op(Op::Join { ask, id: id.to_owned() });
        // SAFETY: the lobby interface is the platform's; the options are copied by the call.
        unsafe { (p.sdk.join_lobby_by_id)(p.lobby_api, &o, cd, on_lobby_id) };
        ask
    }

    fn leave_lobby(&mut self, id: &str) -> Ask {
        let mut p = self.p();
        let ask = p.ask();
        // (shutdown leaves its lobbies itself, after closing)
        if p.closed || p.phase != Phase::Ready {
            p.answer(ask, Ok(Answer::Done));
            return ask;
        }
        if let Err(Left::Answer(ask, r)) = p.leave(id, Left::Answer(ask, Ok(Answer::Done))) {
            p.answer(ask, r);
        }
        ask
    }

    fn kick(&mut self, lobby: &str, member: &str) {
        let mut p = self.p();
        if p.open().is_err() {
            return;
        }
        let l = cstr(lobby);
        let target = p.id_of(member);
        let o = sdk::KickMemberOptions {
            api_version: 1,
            lobby_id: l.as_ptr(),
            local_user_id: p.local,
            target_user_id: target,
        };
        let cd = p.op(Op::Kick);
        // SAFETY: the lobby interface is the platform's; the options are copied by the call.
        unsafe { (p.sdk.kick_member)(p.lobby_api, &o, cd, on_lobby_id) };
    }

    fn send(&mut self, to: &str, channel: Channel, bytes: &[u8]) -> bool {
        let mut p = self.p();
        if p.open().is_err() || bytes.len() > P2P_MAX_PACKET {
            return false;
        }
        if !p.relayed.contains_key(to) {
            p.opening(to);
        }
        let remote = p.id_of(to);
        let socket = sdk::socket_id();
        let reliability = match channel {
            Channel::Match => sdk::PR_UNRELIABLE_UNORDERED,
            Channel::Messages => sdk::PR_RELIABLE_ORDERED,
        };
        let o = sdk::SendPacketOptions {
            api_version: 3,
            local_user_id: p.local,
            remote_user_id: remote,
            socket_id: &socket,
            channel: channel as u8,
            data_length_bytes: u32::try_from(bytes.len()).unwrap_or(0),
            data: bytes.as_ptr().cast(),
            allow_delayed_delivery: 1,
            reliability,
            disable_auto_accept_connection: 0,
        };
        // SAFETY: the P2P interface is the platform's; the options, the socket id and the bytes live through the
        // call, which copies the packet.
        let r = unsafe { (p.sdk.send_packet)(p.p2p, &o) };
        // a tick at the next poll, so the send goes out at once
        // The tick comes at the service's next poll (next_due says now).
        p.last_active = p.now;
        p.flush = true;
        r == sdk::SUCCESS
    }

    fn close_peer(&mut self, peer: &str) {
        let mut p = self.p();
        if p.closed || p.phase == Phase::Dead {
            return;
        }
        p.peer_call(peer, false);
        p.gone(peer);
    }

    fn relayed(&self, peer: &str) -> Option<bool> {
        self.p().relayed.get(peer).copied()
    }

    fn linked_peers(&self) -> Vec<String> {
        self.p().relayed.keys().cloned().collect()
    }

    fn set_relay_mode(&mut self, mode: RelayMode) -> Result<(), String> {
        let mut p = self.p();
        if p.released || p.phase == Phase::Dead {
            return Err("EOS is shut down".to_owned());
        }
        match p.relay_control(mode) {
            Ok(()) => {
                p.mode = Some(mode);
                Ok(())
            }
            Err(r) => {
                p.mode = None;
                Err(format!("EOS relay setting: {}", p.text(r)))
            }
        }
    }

    fn set_accept(&mut self, ids: IndexSet<String>) {
        self.p().accept = ids;
    }

    fn invite(&mut self, to: &str, payload: &str) -> Ask {
        let mut p = self.p();
        let ask = p.ask();
        if let Err(e) = p.open() {
            p.answer(ask, Err(e));
            return ask;
        }
        if payload.len() >= sdk::MAX_INVITE_PAYLOAD {
            p.answer(ask, Err("invite payload too long".to_owned()));
            return ask;
        }
        let text = cstr(payload);
        let so = sdk::SetCustomInviteOptions { api_version: 1, local_user_id: p.local, payload: text.as_ptr() };
        // SAFETY: the invites interface is the platform's; the options and the payload live through the call.
        let set = unsafe { (p.sdk.set_custom_invite)(p.invites, &so) };
        if set != sdk::SUCCESS {
            let e = p.text(set);
            p.answer(ask, Err(e));
            return ask;
        }
        let targets = [p.id_of(to)];
        let o = sdk::SendCustomInviteOptions {
            api_version: 1,
            local_user_id: p.local,
            target_user_ids: targets.as_ptr(),
            target_user_ids_count: 1,
        };
        let cd = p.op(Op::Invite { ask });
        // SAFETY: as above; the options and the target list are copied by the call.
        unsafe { (p.sdk.send_custom_invite)(p.invites, &o, cd, on_send_invite) };
        ask
    }

    fn presence(&mut self) -> Option<&mut dyn Presence> {
        None
    }

    fn poll(&mut self, now: f64) -> Option<BackendEvent> {
        let mut p = lock(&self.p);
        p.now = now;
        p.pump();
        let e = p.events.pop_front();
        if matches!(e, Some(BackendEvent::SignedIn)) {
            self.me = p.me.clone();
        }
        e
    }

    fn next_due(&self) -> Option<f64> {
        self.p().next_due()
    }

    fn shutdown(&mut self) -> Ask {
        self.p().shutdown()
    }
}
