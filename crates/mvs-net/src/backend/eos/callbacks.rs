//! The SDK's callbacks: each decodes its info while EOS's memory holds it and
//! leaves a note in the inbox, which the platform takes after each tick.

use std::ffi::c_void;
use std::sync::Mutex;

use super::sdk::{self, Handle};
use super::{SDK, id_text, lock, text_of};

/// What the SDK's callbacks told, in their order, taken after each tick.
pub(super) static INBOX: Mutex<Vec<Note>> = Mutex::new(Vec::new());

/// What a one-shot call came to.
pub(super) enum Info {
    Result(sdk::EResult),
    Login { code: sdk::EResult, local: usize, continuance: usize },
    CreateUser { code: sdk::EResult, local: usize },
    LobbyId { code: sdk::EResult, lobby: String },
}

pub(super) enum Note {
    Done { token: u64, info: Info },
    AuthExpiration,
    LoginStatus { user: String, status: i32 },
    LobbyUpdate(String),
    MemberStatus { lobby: String, target: String, status: i32 },
    Request(String),
    Established { peer: String, network: i32 },
    Closed(String),
    Invite { from: String, payload: String },
}

fn note(n: Note) {
    lock(&INBOX).push(n);
}

/// A result that is not final (EOS_EResult_IsOperationComplete false: EOS_OperationWillRetry) is skipped: EOS calls
/// the callback again.
fn complete(r: sdk::EResult) -> bool {
    // SAFETY: EOS_EResult_IsOperationComplete takes any value.
    SDK.get().is_none_or(|sdk| unsafe { (sdk.is_operation_complete)(r) } != 0)
}

fn done(client_data: *mut c_void, info: Info) {
    note(Note::Done { token: client_data.addr() as u64, info });
}

// SAFETY (every callback below): EOS calls it with an info that is valid until it returns; nothing of it is kept.

pub(super) unsafe extern "C" fn on_result(i: *const sdk::ResultCallbackInfo) {
    let i = unsafe { &*i };
    if complete(i.result_code) {
        done(i.client_data, Info::Result(i.result_code));
    }
}

pub(super) unsafe extern "C" fn on_login(i: *const sdk::LoginCallbackInfo) {
    let i = unsafe { &*i };
    if complete(i.result_code) {
        let info =
            Info::Login { code: i.result_code, local: i.local_user_id.addr(), continuance: i.continuance_token.addr() };
        done(i.client_data, info);
    }
}

pub(super) unsafe extern "C" fn on_create_user(i: *const sdk::CreateUserCallbackInfo) {
    let i = unsafe { &*i };
    if complete(i.result_code) {
        done(i.client_data, Info::CreateUser { code: i.result_code, local: i.local_user_id.addr() });
    }
}

pub(super) unsafe extern "C" fn on_lobby_id(i: *const sdk::LobbyIdCallbackInfo) {
    let i = unsafe { &*i };
    if complete(i.result_code) {
        let lobby = unsafe { text_of(i.lobby_id) };
        done(i.client_data, Info::LobbyId { code: i.result_code, lobby });
    }
}

pub(super) unsafe extern "C" fn on_send_invite(i: *const sdk::SendCustomInviteCallbackInfo) {
    let i = unsafe { &*i };
    if complete(i.result_code) {
        done(i.client_data, Info::Result(i.result_code));
    }
}

pub(super) unsafe extern "C" fn on_auth_expiration(_: *const sdk::AuthExpirationCallbackInfo) {
    note(Note::AuthExpiration);
}

pub(super) unsafe extern "C" fn on_login_status(i: *const sdk::LoginStatusChangedCallbackInfo) {
    let i = unsafe { &*i };
    if let Some(sdk) = SDK.get() {
        note(Note::LoginStatus { user: id_text(sdk, i.local_user_id), status: i.current_status });
    }
}

pub(super) unsafe extern "C" fn on_lobby_update(i: *const sdk::LobbyUpdateReceivedCallbackInfo) {
    let i = unsafe { &*i };
    note(Note::LobbyUpdate(unsafe { text_of(i.lobby_id) }));
}

pub(super) unsafe extern "C" fn on_member_status(i: *const sdk::LobbyMemberStatusReceivedCallbackInfo) {
    let i = unsafe { &*i };
    if let Some(sdk) = SDK.get() {
        let lobby = unsafe { text_of(i.lobby_id) };
        note(Note::MemberStatus { lobby, target: id_text(sdk, i.target_user_id), status: i.current_status });
    }
}

pub(super) unsafe extern "C" fn on_request(i: *const sdk::IncomingConnectionRequestInfo) {
    let i = unsafe { &*i };
    if let Some(sdk) = SDK.get() {
        note(Note::Request(id_text(sdk, i.remote_user_id)));
    }
}

pub(super) unsafe extern "C" fn on_established(i: *const sdk::PeerConnectionEstablishedInfo) {
    let i = unsafe { &*i };
    if let Some(sdk) = SDK.get() {
        note(Note::Established { peer: id_text(sdk, i.remote_user_id), network: i.network_type });
    }
}

pub(super) unsafe extern "C" fn on_closed(i: *const sdk::RemoteConnectionClosedInfo) {
    let i = unsafe { &*i };
    if let Some(sdk) = SDK.get() {
        note(Note::Closed(id_text(sdk, i.remote_user_id)));
    }
}

/// What the invite callback finalizes with (its client data).
pub(super) struct InviteCtx {
    pub(super) invites: Handle,
    pub(super) local: Handle,
}

pub(super) unsafe extern "C" fn on_invite(i: *const sdk::CustomInviteReceivedCallbackInfo) {
    let i = unsafe { &*i };
    let Some(sdk) = SDK.get() else { return };
    let payload = unsafe { text_of(i.payload) };
    note(Note::Invite { from: id_text(sdk, i.target_user_id), payload });
    // Each received invite is finalized as handled, whatever the service does with it.
    // SAFETY: the client data is the platform's boxed InviteCtx, kept until this notification is removed.
    let ctx = unsafe { &*i.client_data.cast::<InviteCtx>() };
    let o = sdk::FinalizeInviteOptions {
        api_version: 1,
        target_user_id: i.target_user_id,
        local_user_id: ctx.local,
        custom_invite_id: i.custom_invite_id,
        processing_result: sdk::SUCCESS,
    };
    // SAFETY: the handles are the platform's; the invite id is the info's, valid through the callback.
    unsafe { (sdk.finalize_invite)(ctx.invites, &o) };
}
