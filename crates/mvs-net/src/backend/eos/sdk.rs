//! The EOS SDK's C API as far as the backend calls it (EOSSDK-Win64-Shipping.dll, the eos_*.h headers): the result
//! codes and enum values used by name, the options and callback-info structs restated `#[repr(C)]`, and the DLL's
//! functions bound at run time. Each options struct's ApiVersion is the one the SDK's headers pair with it.
//!
//! The layouts are C's natural alignment on x64; each is pinned below by its size and its
//! fields' offsets. Every pin, the ApiVersions and the constants below were checked against the EOS C SDK 1.19.2's
//! eos_*.h (sizeof / offsetof of each struct by its C field names, compiled with MSVC) and agree; the backend runs
//! against that SDK's DLL (tests/eos_pair.rs).

use std::ffi::{c_char, c_void};
use std::mem::{offset_of, size_of};
use std::path::{Path, PathBuf};

use libloading::Library;

/// An EOS handle or id (EOS_HPlatform, EOS_ProductUserId, a lobby details or modification handle).
pub type Handle = *mut c_void;
/// EOS_Bool.
pub type Bool = i32;
/// EOS_EResult.
pub type EResult = i32;
/// EOS_NotificationId (0: EOS_INVALID_NOTIFICATIONID).
pub type NotificationId = u64;

// EOS_EResult values used by name (eos_result.h).
pub const SUCCESS: EResult = 0;
pub const INVALID_USER: EResult = 3;
pub const NOT_FOUND: EResult = 18;
pub const NO_CHANGE: EResult = 20;
pub const DUPLICATE_NOT_ALLOWED: EResult = 24;

// EOS_ELobbyMemberStatus.
pub const LMS_KICKED: i32 = 3;
pub const LMS_LEFT: i32 = 1;
pub const LMS_DISCONNECTED: i32 = 2;
pub const LMS_CLOSED: i32 = 5;
/// EOS_ENetworkConnectionType: EOS_NCT_RelayedConnection.
pub const NCT_RELAYED: i32 = 2;
/// EOS_ERelayControl: EOS_RC_AllowRelays, EOS_RC_ForceRelays.
pub const RC_ALLOW_RELAYS: i32 = 1;
pub const RC_FORCE_RELAYS: i32 = 2;
// EOS_EPacketReliability.
pub const PR_UNRELIABLE_UNORDERED: i32 = 0;
pub const PR_RELIABLE_ORDERED: i32 = 2;
// EOS_ELobbyPermissionLevel.
pub const LPL_PUBLICADVERTISED: i32 = 0;
pub const LPL_INVITEONLY: i32 = 2;
/// EOS_ELobbyAttributeVisibility: EOS_LAT_PUBLIC (searchers read it).
pub const LAT_PUBLIC: i32 = 0;
// EOS_EAttributeType.
pub const AT_INT64: i32 = 1;
pub const AT_STRING: i32 = 3;
// EOS_EComparisonOp.
pub const CO_EQUAL: i32 = 0;
pub const CO_GREATERTHANOREQUAL: i32 = 3;
/// EOS_LOBBY_SEARCH_BUCKET_ID: the search key of a lobby's bucket.
pub const SEARCH_BUCKET: &str = "bucket";
/// EOS_LOBBY_SEARCH_MINSLOTSAVAILABLE.
pub const SEARCH_MIN_SLOTS: &str = "minslotsavailable";
/// EOS_LOBBY_MAX_SEARCH_RESULTS.
pub const MAX_SEARCH_RESULTS: u32 = 200;
/// EOS_EExternalCredentialType: EOS_ECT_DEVICEID_ACCESS_TOKEN.
pub const ECT_DEVICEID_ACCESS_TOKEN: i32 = 10;
/// EOS_ELoginStatus: EOS_LS_NotLoggedIn.
pub const LS_NOT_LOGGED_IN: i32 = 0;
/// EOS_PF_DISABLE_OVERLAY: no overlay in a process with no window.
pub const PF_DISABLE_OVERLAY: u64 = 0x2;
/// EOS_CUSTOMINVITES_MAX_PAYLOAD_LENGTH: a payload is shorter.
pub const MAX_INVITE_PAYLOAD: usize = 500;
/// EOS_PRODUCTUSERID_MAX_LENGTH and its null.
pub const USER_ID_BUFFER: usize = 33;
/// EOS_P2P_SocketId's SocketName: 32 characters and the null.
pub const SOCKET_NAME_BYTES: usize = 33;

/// Pins a struct's size and its fields' offsets at compile time.
macro_rules! layout {
    ($t:ty, $size:expr; $($field:ident @ $off:expr),* $(,)?) => {
        const _: () = assert!(size_of::<$t>() == $size);
        $(const _: () = assert!(offset_of!($t, $field) == $off);)*
    };
}

// ---- platform

/// EOS_InitializeOptions (ApiVersion 5).
#[repr(C)]
pub struct InitializeOptions {
    pub api_version: i32,
    pub allocate_memory_function: *mut c_void,
    pub reallocate_memory_function: *mut c_void,
    pub release_memory_function: *mut c_void,
    pub product_name: *const c_char,
    pub product_version: *const c_char,
    pub reserved: *mut c_void,
    pub system_initialize_options: *mut c_void,
    pub override_thread_affinity: *mut c_void,
}
layout!(InitializeOptions, 72; api_version @ 0, allocate_memory_function @ 8, reallocate_memory_function @ 16,
    release_memory_function @ 24, product_name @ 32, product_version @ 40, reserved @ 48,
    system_initialize_options @ 56, override_thread_affinity @ 64);

/// EOS_Platform_ClientCredentials.
#[repr(C)]
pub struct ClientCredentials {
    pub client_id: *const c_char,
    pub client_secret: *const c_char,
}
layout!(ClientCredentials, 16; client_id @ 0, client_secret @ 8);

/// EOS_Platform_Options (ApiVersion 15).
#[repr(C)]
pub struct PlatformOptions {
    pub api_version: i32,
    pub reserved: *mut c_void,
    pub product_id: *const c_char,
    pub sandbox_id: *const c_char,
    pub client_credentials: ClientCredentials,
    pub is_server: Bool,
    pub encryption_key: *const c_char,
    pub override_country_code: *const c_char,
    pub override_locale_code: *const c_char,
    pub deployment_id: *const c_char,
    pub flags: u64,
    pub cache_directory: *const c_char,
    pub tick_budget_in_milliseconds: u32,
    pub rtc_options: *mut c_void,
    pub integrated_platform_options_container_handle: *mut c_void,
    pub system_specific_options: *mut c_void,
    pub task_network_timeout_seconds: *mut c_void,
}
layout!(PlatformOptions, 144; api_version @ 0, reserved @ 8, product_id @ 16, sandbox_id @ 24,
    client_credentials @ 32, is_server @ 48, encryption_key @ 56, override_country_code @ 64,
    override_locale_code @ 72, deployment_id @ 80, flags @ 88, cache_directory @ 96,
    tick_budget_in_milliseconds @ 104, rtc_options @ 112, integrated_platform_options_container_handle @ 120,
    system_specific_options @ 128, task_network_timeout_seconds @ 136);

/// The options of every call that takes only its version: EOS_LobbyDetails_GetLobbyOwner, _GetMemberCount,
/// _CopyInfo, EOS_LobbySearch_GetSearchResultCount and the Add-notify calls of the lobby and invite interfaces.
#[repr(C)]
pub struct ApiVersionOptions {
    pub api_version: i32,
}
layout!(ApiVersionOptions, 4; api_version @ 0);

// ---- connect: Device ID sign-in

/// EOS_Connect_CreateDeviceIdOptions (ApiVersion 1).
#[repr(C)]
pub struct CreateDeviceIdOptions {
    pub api_version: i32,
    pub device_model: *const c_char,
}
layout!(CreateDeviceIdOptions, 16; api_version @ 0, device_model @ 8);

/// EOS_Connect_DeleteDeviceIdOptions (ApiVersion 1).
#[repr(C)]
pub struct DeleteDeviceIdOptions {
    pub api_version: i32,
}
layout!(DeleteDeviceIdOptions, 4; api_version @ 0);

/// EOS_Connect_CreateDeviceIdCallbackInfo; also read for DeleteDeviceId's and EOS_LobbySearch_Find's (a result and
/// the client data first).
#[repr(C)]
pub struct ResultCallbackInfo {
    pub result_code: EResult,
    pub client_data: *mut c_void,
}
layout!(ResultCallbackInfo, 16; result_code @ 0, client_data @ 8);

/// EOS_Connect_Credentials (ApiVersion 1).
#[repr(C)]
pub struct ConnectCredentials {
    pub api_version: i32,
    pub token: *const c_char,
    pub kind: i32,
}
layout!(ConnectCredentials, 24; api_version @ 0, token @ 8, kind @ 16);

/// EOS_Connect_UserLoginInfo (ApiVersion 2).
#[repr(C)]
pub struct UserLoginInfo {
    pub api_version: i32,
    pub display_name: *const c_char,
    pub nsa_id_token: *const c_char,
}
layout!(UserLoginInfo, 24; api_version @ 0, display_name @ 8, nsa_id_token @ 16);

/// EOS_Connect_LoginOptions (ApiVersion 2).
#[repr(C)]
pub struct LoginOptions {
    pub api_version: i32,
    pub credentials: *const ConnectCredentials,
    pub user_login_info: *const UserLoginInfo,
}
layout!(LoginOptions, 24; api_version @ 0, credentials @ 8, user_login_info @ 16);

/// EOS_Connect_LoginCallbackInfo.
#[repr(C)]
pub struct LoginCallbackInfo {
    pub result_code: EResult,
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
    pub continuance_token: Handle,
}
layout!(LoginCallbackInfo, 32; result_code @ 0, client_data @ 8, local_user_id @ 16, continuance_token @ 24);

/// EOS_Connect_CreateUserOptions (ApiVersion 1).
#[repr(C)]
pub struct CreateUserOptions {
    pub api_version: i32,
    pub continuance_token: Handle,
}
layout!(CreateUserOptions, 16; api_version @ 0, continuance_token @ 8);

/// EOS_Connect_CreateUserCallbackInfo.
#[repr(C)]
pub struct CreateUserCallbackInfo {
    pub result_code: EResult,
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
}
layout!(CreateUserCallbackInfo, 24; result_code @ 0, client_data @ 8, local_user_id @ 16);

/// EOS_Connect_AddNotifyAuthExpirationOptions and _AddNotifyLoginStatusChangedOptions (ApiVersion 1).
#[repr(C)]
pub struct AddNotifyOptions {
    pub api_version: i32,
}
layout!(AddNotifyOptions, 4; api_version @ 0);

/// EOS_Connect_AuthExpirationCallbackInfo.
#[repr(C)]
pub struct AuthExpirationCallbackInfo {
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
}
layout!(AuthExpirationCallbackInfo, 16; client_data @ 0, local_user_id @ 8);

/// EOS_Connect_LoginStatusChangedCallbackInfo.
#[repr(C)]
pub struct LoginStatusChangedCallbackInfo {
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
    pub previous_status: i32,
    pub current_status: i32,
}
layout!(LoginStatusChangedCallbackInfo, 24; client_data @ 0, local_user_id @ 8, previous_status @ 16,
    current_status @ 20);

// ---- lobby

/// EOS_Lobby_CreateLobbyOptions (ApiVersion 10).
#[repr(C)]
pub struct CreateLobbyOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub max_lobby_members: u32,
    pub permission_level: i32,
    pub presence_enabled: Bool,
    pub allow_invites: Bool,
    pub bucket_id: *const c_char,
    pub disable_host_migration: Bool,
    pub enable_rtc_room: Bool,
    pub local_rtc_options: *mut c_void,
    pub lobby_id: *const c_char,
    pub enable_join_by_id: Bool,
    pub rejoin_after_kick_requires_invite: Bool,
    pub allowed_platform_ids: *mut c_void,
    pub allowed_platform_ids_count: u32,
    pub crossplay_opt_out: Bool,
    pub rtc_room_join_action_type: i32,
}
layout!(CreateLobbyOptions, 96; api_version @ 0, local_user_id @ 8, max_lobby_members @ 16, permission_level @ 20,
    presence_enabled @ 24, allow_invites @ 28, bucket_id @ 32, disable_host_migration @ 40, enable_rtc_room @ 44,
    local_rtc_options @ 48, lobby_id @ 56, enable_join_by_id @ 64, rejoin_after_kick_requires_invite @ 68,
    allowed_platform_ids @ 72, allowed_platform_ids_count @ 80, crossplay_opt_out @ 84,
    rtc_room_join_action_type @ 88);

/// EOS_Lobby_CreateLobbyCallbackInfo, and JoinLobbyById's, LeaveLobby's, DestroyLobby's, KickMember's and
/// UpdateLobby's: one layout.
#[repr(C)]
pub struct LobbyIdCallbackInfo {
    pub result_code: EResult,
    pub client_data: *mut c_void,
    pub lobby_id: *const c_char,
}
layout!(LobbyIdCallbackInfo, 24; result_code @ 0, client_data @ 8, lobby_id @ 16);

/// EOS_Lobby_JoinLobbyByIdOptions (ApiVersion 3).
#[repr(C)]
pub struct JoinLobbyByIdOptions {
    pub api_version: i32,
    pub lobby_id: *const c_char,
    pub local_user_id: Handle,
    pub presence_enabled: Bool,
    pub local_rtc_options: *mut c_void,
    pub crossplay_opt_out: Bool,
    pub rtc_room_join_action_type: i32,
}
layout!(JoinLobbyByIdOptions, 48; api_version @ 0, lobby_id @ 8, local_user_id @ 16, presence_enabled @ 24,
    local_rtc_options @ 32, crossplay_opt_out @ 40, rtc_room_join_action_type @ 44);

/// EOS_Lobby_LeaveLobbyOptions and EOS_Lobby_DestroyLobbyOptions (ApiVersion 1): one layout.
#[repr(C)]
pub struct LeaveLobbyOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub lobby_id: *const c_char,
}
layout!(LeaveLobbyOptions, 24; api_version @ 0, local_user_id @ 8, lobby_id @ 16);

/// EOS_Lobby_KickMemberOptions (ApiVersion 1).
#[repr(C)]
pub struct KickMemberOptions {
    pub api_version: i32,
    pub lobby_id: *const c_char,
    pub local_user_id: Handle,
    pub target_user_id: Handle,
}
layout!(KickMemberOptions, 32; api_version @ 0, lobby_id @ 8, local_user_id @ 16, target_user_id @ 24);

/// EOS_Lobby_CopyLobbyDetailsHandleOptions (ApiVersion 1).
#[repr(C)]
pub struct CopyLobbyDetailsHandleOptions {
    pub api_version: i32,
    pub lobby_id: *const c_char,
    pub local_user_id: Handle,
}
layout!(CopyLobbyDetailsHandleOptions, 24; api_version @ 0, lobby_id @ 8, local_user_id @ 16);

/// EOS_LobbyDetails_GetMemberByIndexOptions (ApiVersion 1).
#[repr(C)]
pub struct GetMemberByIndexOptions {
    pub api_version: i32,
    pub member_index: u32,
}
layout!(GetMemberByIndexOptions, 8; api_version @ 0, member_index @ 4);

/// EOS_Lobby_LobbyUpdateReceivedCallbackInfo.
#[repr(C)]
pub struct LobbyUpdateReceivedCallbackInfo {
    pub client_data: *mut c_void,
    pub lobby_id: *const c_char,
}
layout!(LobbyUpdateReceivedCallbackInfo, 16; client_data @ 0, lobby_id @ 8);

/// EOS_Lobby_LobbyMemberStatusReceivedCallbackInfo.
#[repr(C)]
pub struct LobbyMemberStatusReceivedCallbackInfo {
    pub client_data: *mut c_void,
    pub lobby_id: *const c_char,
    pub target_user_id: Handle,
    pub current_status: i32,
}
layout!(LobbyMemberStatusReceivedCallbackInfo, 32; client_data @ 0, lobby_id @ 8, target_user_id @ 16,
    current_status @ 24);

/// EOS_Lobby_AttributeData's value (the union EOS_Lobby_AttributeData_Value), read by its ValueType.
#[repr(C)]
#[derive(Clone, Copy)]
pub union AttributeValue {
    pub as_int64: i64,
    pub as_double: f64,
    pub as_bool: Bool,
    pub as_utf8: *const c_char,
}
layout!(AttributeValue, 8;);

/// EOS_Lobby_AttributeData (ApiVersion 1). Its union (AsInt64, AsUtf8) is read here by ValueType.
#[repr(C)]
pub struct AttributeData {
    pub api_version: i32,
    pub key: *const c_char,
    pub value: AttributeValue,
    pub value_type: i32,
}
layout!(AttributeData, 32; api_version @ 0, key @ 8, value @ 16, value_type @ 24);

/// EOS_Lobby_Attribute.
#[repr(C)]
pub struct Attribute {
    pub api_version: i32,
    pub data: *const AttributeData,
    pub visibility: i32,
}
layout!(Attribute, 24; api_version @ 0, data @ 8, visibility @ 16);

/// EOS_Lobby_UpdateLobbyModificationOptions (ApiVersion 1).
#[repr(C)]
pub struct UpdateLobbyModificationOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub lobby_id: *const c_char,
}
layout!(UpdateLobbyModificationOptions, 24; api_version @ 0, local_user_id @ 8, lobby_id @ 16);

/// EOS_Lobby_UpdateLobbyOptions (ApiVersion 1).
#[repr(C)]
pub struct UpdateLobbyOptions {
    pub api_version: i32,
    pub lobby_modification_handle: Handle,
}
layout!(UpdateLobbyOptions, 16; api_version @ 0, lobby_modification_handle @ 8);

/// EOS_LobbyModification_SetPermissionLevelOptions (ApiVersion 1).
#[repr(C)]
pub struct SetPermissionLevelOptions {
    pub api_version: i32,
    pub permission_level: i32,
}
layout!(SetPermissionLevelOptions, 8; api_version @ 0, permission_level @ 4);

/// EOS_LobbyModification_AddAttributeOptions (ApiVersion 2).
#[repr(C)]
pub struct AddAttributeOptions {
    pub api_version: i32,
    pub attribute: *const AttributeData,
    pub visibility: i32,
}
layout!(AddAttributeOptions, 24; api_version @ 0, attribute @ 8, visibility @ 16);

/// EOS_Lobby_CreateLobbySearchOptions (ApiVersion 1).
#[repr(C)]
pub struct CreateLobbySearchOptions {
    pub api_version: i32,
    pub max_results: u32,
}
layout!(CreateLobbySearchOptions, 8; api_version @ 0, max_results @ 4);

/// EOS_LobbySearch_SetParameterOptions (ApiVersion 1).
#[repr(C)]
pub struct SetParameterOptions {
    pub api_version: i32,
    pub parameter: *const AttributeData,
    pub comparison_op: i32,
}
layout!(SetParameterOptions, 24; api_version @ 0, parameter @ 8, comparison_op @ 16);

/// EOS_LobbySearch_FindOptions (ApiVersion 1).
#[repr(C)]
pub struct FindOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
}
layout!(FindOptions, 16; api_version @ 0, local_user_id @ 8);

/// EOS_LobbySearch_CopySearchResultByIndexOptions (ApiVersion 1).
#[repr(C)]
pub struct CopySearchResultByIndexOptions {
    pub api_version: i32,
    pub lobby_index: u32,
}
layout!(CopySearchResultByIndexOptions, 8; api_version @ 0, lobby_index @ 4);

/// EOS_LobbyDetails_CopyAttributeByKeyOptions (ApiVersion 1).
#[repr(C)]
pub struct CopyAttributeByKeyOptions {
    pub api_version: i32,
    pub attr_key: *const c_char,
}
layout!(CopyAttributeByKeyOptions, 16; api_version @ 0, attr_key @ 8);

/// EOS_LobbyDetails_Info.
#[repr(C)]
pub struct LobbyDetailsInfo {
    pub api_version: i32,
    pub lobby_id: *const c_char,
    pub lobby_owner_user_id: Handle,
    pub permission_level: i32,
    pub available_slots: u32,
    pub max_members: u32,
    pub allow_invites: Bool,
    pub bucket_id: *const c_char,
    pub allow_host_migration: Bool,
    pub rtc_room_enabled: Bool,
    pub allow_join_by_id: Bool,
    pub rejoin_after_kick_requires_invite: Bool,
    pub presence_enabled: Bool,
    pub allowed_platform_ids: *mut c_void,
    pub allowed_platform_ids_count: u32,
}
layout!(LobbyDetailsInfo, 88; api_version @ 0, lobby_id @ 8, lobby_owner_user_id @ 16, permission_level @ 24,
    available_slots @ 28, max_members @ 32, allow_invites @ 36, bucket_id @ 40, allow_host_migration @ 48,
    rtc_room_enabled @ 52, allow_join_by_id @ 56, rejoin_after_kick_requires_invite @ 60, presence_enabled @ 64,
    allowed_platform_ids @ 72, allowed_platform_ids_count @ 80);

// ---- P2P

/// EOS_P2P_SocketId (ApiVersion 1).
#[repr(C)]
pub struct SocketId {
    pub api_version: i32,
    pub socket_name: [c_char; SOCKET_NAME_BYTES],
}
layout!(SocketId, 40; api_version @ 0, socket_name @ 4);

/// EOS_P2P_SendPacketOptions (ApiVersion 3).
#[repr(C)]
pub struct SendPacketOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub remote_user_id: Handle,
    pub socket_id: *const SocketId,
    pub channel: u8,
    pub data_length_bytes: u32,
    pub data: *const c_void,
    pub allow_delayed_delivery: Bool,
    pub reliability: i32,
    pub disable_auto_accept_connection: Bool,
}
layout!(SendPacketOptions, 64; api_version @ 0, local_user_id @ 8, remote_user_id @ 16, socket_id @ 24,
    channel @ 32, data_length_bytes @ 36, data @ 40, allow_delayed_delivery @ 48, reliability @ 52,
    disable_auto_accept_connection @ 56);

/// EOS_P2P_GetNextReceivedPacketSizeOptions (ApiVersion 2).
#[repr(C)]
pub struct GetNextReceivedPacketSizeOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub requested_channel: *const u8,
}
layout!(GetNextReceivedPacketSizeOptions, 24; api_version @ 0, local_user_id @ 8, requested_channel @ 16);

/// EOS_P2P_ReceivePacketOptions (ApiVersion 2).
#[repr(C)]
pub struct ReceivePacketOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub max_data_size_bytes: u32,
    pub requested_channel: *const u8,
}
layout!(ReceivePacketOptions, 32; api_version @ 0, local_user_id @ 8, max_data_size_bytes @ 16,
    requested_channel @ 24);

/// EOS_P2P_AddNotifyPeerConnectionRequestOptions, _EstablishedOptions and _ClosedOptions (ApiVersion 1): one layout.
#[repr(C)]
pub struct PeerNotifyOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub socket_id: *const SocketId,
}
layout!(PeerNotifyOptions, 24; api_version @ 0, local_user_id @ 8, socket_id @ 16);

/// EOS_P2P_AcceptConnectionOptions and EOS_P2P_CloseConnectionOptions (ApiVersion 1): one layout.
#[repr(C)]
pub struct PeerOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub remote_user_id: Handle,
    pub socket_id: *const SocketId,
}
layout!(PeerOptions, 32; api_version @ 0, local_user_id @ 8, remote_user_id @ 16, socket_id @ 24);

/// EOS_P2P_SetRelayControlOptions (ApiVersion 1).
#[repr(C)]
pub struct SetRelayControlOptions {
    pub api_version: i32,
    pub relay_control: i32,
}
layout!(SetRelayControlOptions, 8; api_version @ 0, relay_control @ 4);

/// EOS_P2P_OnIncomingConnectionRequestInfo.
#[repr(C)]
pub struct IncomingConnectionRequestInfo {
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
    pub remote_user_id: Handle,
    pub socket_id: *const SocketId,
}
layout!(IncomingConnectionRequestInfo, 32; client_data @ 0, local_user_id @ 8, remote_user_id @ 16, socket_id @ 24);

/// EOS_P2P_OnPeerConnectionEstablishedInfo.
#[repr(C)]
pub struct PeerConnectionEstablishedInfo {
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
    pub remote_user_id: Handle,
    pub socket_id: *const SocketId,
    pub connection_type: i32,
    pub network_type: i32,
}
layout!(PeerConnectionEstablishedInfo, 40; client_data @ 0, local_user_id @ 8, remote_user_id @ 16, socket_id @ 24,
    connection_type @ 32, network_type @ 36);

/// EOS_P2P_OnRemoteConnectionClosedInfo.
#[repr(C)]
pub struct RemoteConnectionClosedInfo {
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
    pub remote_user_id: Handle,
    pub socket_id: *const SocketId,
    pub reason: i32,
}
layout!(RemoteConnectionClosedInfo, 40; client_data @ 0, local_user_id @ 8, remote_user_id @ 16, socket_id @ 24,
    reason @ 32);

// ---- custom invites

/// EOS_CustomInvites_SetCustomInviteOptions (ApiVersion 1).
#[repr(C)]
pub struct SetCustomInviteOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub payload: *const c_char,
}
layout!(SetCustomInviteOptions, 24; api_version @ 0, local_user_id @ 8, payload @ 16);

/// EOS_CustomInvites_SendCustomInviteOptions (ApiVersion 1).
#[repr(C)]
pub struct SendCustomInviteOptions {
    pub api_version: i32,
    pub local_user_id: Handle,
    pub target_user_ids: *const Handle,
    pub target_user_ids_count: u32,
}
layout!(SendCustomInviteOptions, 32; api_version @ 0, local_user_id @ 8, target_user_ids @ 16,
    target_user_ids_count @ 24);

/// EOS_CustomInvites_SendCustomInviteCallbackInfo.
#[repr(C)]
pub struct SendCustomInviteCallbackInfo {
    pub result_code: EResult,
    pub client_data: *mut c_void,
    pub local_user_id: Handle,
    pub target_user_ids: *const Handle,
    pub target_user_ids_count: u32,
}
layout!(SendCustomInviteCallbackInfo, 40; result_code @ 0, client_data @ 8, local_user_id @ 16,
    target_user_ids @ 24, target_user_ids_count @ 32);

/// EOS_CustomInvites_OnCustomInviteReceivedCallbackInfo.
#[repr(C)]
pub struct CustomInviteReceivedCallbackInfo {
    pub client_data: *mut c_void,
    pub target_user_id: Handle,
    pub local_user_id: Handle,
    pub custom_invite_id: *const c_char,
    pub payload: *const c_char,
}
layout!(CustomInviteReceivedCallbackInfo, 40; client_data @ 0, target_user_id @ 8, local_user_id @ 16,
    custom_invite_id @ 24, payload @ 32);

/// EOS_CustomInvites_FinalizeInviteOptions (ApiVersion 1).
#[repr(C)]
pub struct FinalizeInviteOptions {
    pub api_version: i32,
    pub target_user_id: Handle,
    pub local_user_id: Handle,
    pub custom_invite_id: *const c_char,
    pub processing_result: EResult,
}
layout!(FinalizeInviteOptions, 40; api_version @ 0, target_user_id @ 8, local_user_id @ 16, custom_invite_id @ 24,
    processing_result @ 32);

// ---- callbacks (EOS_CALL is the platform's C convention on x64)

pub type OnResult = unsafe extern "C" fn(*const ResultCallbackInfo);
pub type OnLogin = unsafe extern "C" fn(*const LoginCallbackInfo);
pub type OnCreateUser = unsafe extern "C" fn(*const CreateUserCallbackInfo);
pub type OnAuthExpiration = unsafe extern "C" fn(*const AuthExpirationCallbackInfo);
pub type OnLoginStatusChanged = unsafe extern "C" fn(*const LoginStatusChangedCallbackInfo);
pub type OnLobbyId = unsafe extern "C" fn(*const LobbyIdCallbackInfo);
pub type OnLobbyUpdate = unsafe extern "C" fn(*const LobbyUpdateReceivedCallbackInfo);
pub type OnMemberStatus = unsafe extern "C" fn(*const LobbyMemberStatusReceivedCallbackInfo);
pub type OnRequest = unsafe extern "C" fn(*const IncomingConnectionRequestInfo);
pub type OnEstablished = unsafe extern "C" fn(*const PeerConnectionEstablishedInfo);
pub type OnClosed = unsafe extern "C" fn(*const RemoteConnectionClosedInfo);
pub type OnSendInvite = unsafe extern "C" fn(*const SendCustomInviteCallbackInfo);
pub type OnInvite = unsafe extern "C" fn(*const CustomInviteReceivedCallbackInfo);

/// Declares the DLL's functions: each a field of `Sdk` bound by its export name in `Sdk::load`.
macro_rules! sdk {
    ($($field:ident = $sym:literal: fn($($arg:ty),* $(,)?) $(-> $ret:ty)?;)*) => {
        /// The SDK's DLL and the functions the backend calls, bound once per process and never unloaded.
        pub struct Sdk {
            _lib: Library,
            pub path: PathBuf,
            $(pub $field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Sdk {
            /// Loads the DLL at `path` and binds every function; the loader's or the missing export's error.
            pub fn load(path: &Path) -> Result<Sdk, String> {
                // SAFETY: loading runs the DLL's initialisers; the path is the EOS SDK the install names, whose
                // initialisers do no more than set the SDK up.
                let lib = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
                $(
                    // SAFETY: the export's C signature is the one declared here (eos_*.h as restated here);
                    // the pointer is copied out and stays valid while `_lib` is held.
                    let $field = unsafe {
                        *lib.get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>($sym).map_err(|e| e.to_string())?
                    };
                )*
                Ok(Sdk { _lib: lib, path: path.to_owned(), $($field,)* })
            }
        }
    };
}

sdk! {
    initialize = "EOS_Initialize": fn(*const InitializeOptions) -> EResult;
    shutdown = "EOS_Shutdown": fn() -> EResult;
    platform_create = "EOS_Platform_Create": fn(*const PlatformOptions) -> Handle;
    platform_tick = "EOS_Platform_Tick": fn(Handle);
    platform_release = "EOS_Platform_Release": fn(Handle);
    result_to_string = "EOS_EResult_ToString": fn(EResult) -> *const c_char;
    is_operation_complete = "EOS_EResult_IsOperationComplete": fn(EResult) -> Bool;
    get_connect = "EOS_Platform_GetConnectInterface": fn(Handle) -> Handle;
    get_lobby = "EOS_Platform_GetLobbyInterface": fn(Handle) -> Handle;
    get_p2p = "EOS_Platform_GetP2PInterface": fn(Handle) -> Handle;
    get_custom_invites = "EOS_Platform_GetCustomInvitesInterface": fn(Handle) -> Handle;
    user_id_to_string = "EOS_ProductUserId_ToString": fn(Handle, *mut c_char, *mut i32) -> EResult;
    user_id_from_string = "EOS_ProductUserId_FromString": fn(*const c_char) -> Handle;

    create_device_id = "EOS_Connect_CreateDeviceId": fn(Handle, *const CreateDeviceIdOptions, *mut c_void, OnResult);
    delete_device_id = "EOS_Connect_DeleteDeviceId": fn(Handle, *const DeleteDeviceIdOptions, *mut c_void, OnResult);
    login = "EOS_Connect_Login": fn(Handle, *const LoginOptions, *mut c_void, OnLogin);
    create_user = "EOS_Connect_CreateUser": fn(Handle, *const CreateUserOptions, *mut c_void, OnCreateUser);
    add_notify_auth_expiration = "EOS_Connect_AddNotifyAuthExpiration":
        fn(Handle, *const AddNotifyOptions, *mut c_void, OnAuthExpiration) -> NotificationId;
    remove_notify_auth_expiration = "EOS_Connect_RemoveNotifyAuthExpiration": fn(Handle, NotificationId);
    add_notify_login_status_changed = "EOS_Connect_AddNotifyLoginStatusChanged":
        fn(Handle, *const AddNotifyOptions, *mut c_void, OnLoginStatusChanged) -> NotificationId;
    remove_notify_login_status_changed = "EOS_Connect_RemoveNotifyLoginStatusChanged": fn(Handle, NotificationId);

    create_lobby = "EOS_Lobby_CreateLobby": fn(Handle, *const CreateLobbyOptions, *mut c_void, OnLobbyId);
    join_lobby_by_id = "EOS_Lobby_JoinLobbyById": fn(Handle, *const JoinLobbyByIdOptions, *mut c_void, OnLobbyId);
    leave_lobby = "EOS_Lobby_LeaveLobby": fn(Handle, *const LeaveLobbyOptions, *mut c_void, OnLobbyId);
    destroy_lobby = "EOS_Lobby_DestroyLobby": fn(Handle, *const LeaveLobbyOptions, *mut c_void, OnLobbyId);
    kick_member = "EOS_Lobby_KickMember": fn(Handle, *const KickMemberOptions, *mut c_void, OnLobbyId);
    copy_lobby_details = "EOS_Lobby_CopyLobbyDetailsHandle":
        fn(Handle, *const CopyLobbyDetailsHandleOptions, *mut Handle) -> EResult;
    get_lobby_owner = "EOS_LobbyDetails_GetLobbyOwner": fn(Handle, *const ApiVersionOptions) -> Handle;
    get_member_count = "EOS_LobbyDetails_GetMemberCount": fn(Handle, *const ApiVersionOptions) -> u32;
    get_member_by_index = "EOS_LobbyDetails_GetMemberByIndex": fn(Handle, *const GetMemberByIndexOptions) -> Handle;
    lobby_details_release = "EOS_LobbyDetails_Release": fn(Handle);
    add_notify_lobby_update = "EOS_Lobby_AddNotifyLobbyUpdateReceived":
        fn(Handle, *const ApiVersionOptions, *mut c_void, OnLobbyUpdate) -> NotificationId;
    remove_notify_lobby_update = "EOS_Lobby_RemoveNotifyLobbyUpdateReceived": fn(Handle, NotificationId);
    add_notify_member_status = "EOS_Lobby_AddNotifyLobbyMemberStatusReceived":
        fn(Handle, *const ApiVersionOptions, *mut c_void, OnMemberStatus) -> NotificationId;
    remove_notify_member_status = "EOS_Lobby_RemoveNotifyLobbyMemberStatusReceived": fn(Handle, NotificationId);
    update_lobby_modification = "EOS_Lobby_UpdateLobbyModification":
        fn(Handle, *const UpdateLobbyModificationOptions, *mut Handle) -> EResult;
    modification_set_permission_level = "EOS_LobbyModification_SetPermissionLevel":
        fn(Handle, *const SetPermissionLevelOptions) -> EResult;
    modification_add_attribute = "EOS_LobbyModification_AddAttribute":
        fn(Handle, *const AddAttributeOptions) -> EResult;
    modification_release = "EOS_LobbyModification_Release": fn(Handle);
    update_lobby = "EOS_Lobby_UpdateLobby": fn(Handle, *const UpdateLobbyOptions, *mut c_void, OnLobbyId);
    create_lobby_search = "EOS_Lobby_CreateLobbySearch":
        fn(Handle, *const CreateLobbySearchOptions, *mut Handle) -> EResult;
    search_set_parameter = "EOS_LobbySearch_SetParameter": fn(Handle, *const SetParameterOptions) -> EResult;
    search_find = "EOS_LobbySearch_Find": fn(Handle, *const FindOptions, *mut c_void, OnResult);
    search_result_count = "EOS_LobbySearch_GetSearchResultCount": fn(Handle, *const ApiVersionOptions) -> u32;
    search_result = "EOS_LobbySearch_CopySearchResultByIndex":
        fn(Handle, *const CopySearchResultByIndexOptions, *mut Handle) -> EResult;
    search_release = "EOS_LobbySearch_Release": fn(Handle);
    details_copy_info = "EOS_LobbyDetails_CopyInfo":
        fn(Handle, *const ApiVersionOptions, *mut *mut LobbyDetailsInfo) -> EResult;
    details_info_release = "EOS_LobbyDetails_Info_Release": fn(*mut LobbyDetailsInfo);
    details_copy_attribute = "EOS_LobbyDetails_CopyAttributeByKey":
        fn(Handle, *const CopyAttributeByKeyOptions, *mut *mut Attribute) -> EResult;
    attribute_release = "EOS_Lobby_Attribute_Release": fn(*mut Attribute);

    set_relay_control = "EOS_P2P_SetRelayControl": fn(Handle, *const SetRelayControlOptions) -> EResult;
    send_packet = "EOS_P2P_SendPacket": fn(Handle, *const SendPacketOptions) -> EResult;
    next_packet_size = "EOS_P2P_GetNextReceivedPacketSize":
        fn(Handle, *const GetNextReceivedPacketSizeOptions, *mut u32) -> EResult;
    receive_packet = "EOS_P2P_ReceivePacket":
        fn(Handle, *const ReceivePacketOptions, *mut Handle, *mut SocketId, *mut u8, *mut u8, *mut u32) -> EResult;
    accept_connection = "EOS_P2P_AcceptConnection": fn(Handle, *const PeerOptions) -> EResult;
    close_connection = "EOS_P2P_CloseConnection": fn(Handle, *const PeerOptions) -> EResult;
    add_notify_request = "EOS_P2P_AddNotifyPeerConnectionRequest":
        fn(Handle, *const PeerNotifyOptions, *mut c_void, OnRequest) -> NotificationId;
    remove_notify_request = "EOS_P2P_RemoveNotifyPeerConnectionRequest": fn(Handle, NotificationId);
    add_notify_established = "EOS_P2P_AddNotifyPeerConnectionEstablished":
        fn(Handle, *const PeerNotifyOptions, *mut c_void, OnEstablished) -> NotificationId;
    remove_notify_established = "EOS_P2P_RemoveNotifyPeerConnectionEstablished": fn(Handle, NotificationId);
    add_notify_closed = "EOS_P2P_AddNotifyPeerConnectionClosed":
        fn(Handle, *const PeerNotifyOptions, *mut c_void, OnClosed) -> NotificationId;
    remove_notify_closed = "EOS_P2P_RemoveNotifyPeerConnectionClosed": fn(Handle, NotificationId);

    set_custom_invite = "EOS_CustomInvites_SetCustomInvite": fn(Handle, *const SetCustomInviteOptions) -> EResult;
    send_custom_invite = "EOS_CustomInvites_SendCustomInvite":
        fn(Handle, *const SendCustomInviteOptions, *mut c_void, OnSendInvite);
    add_notify_invite = "EOS_CustomInvites_AddNotifyCustomInviteReceived":
        fn(Handle, *const ApiVersionOptions, *mut c_void, OnInvite) -> NotificationId;
    remove_notify_invite = "EOS_CustomInvites_RemoveNotifyCustomInviteReceived": fn(Handle, NotificationId);
    finalize_invite = "EOS_CustomInvites_FinalizeInvite": fn(Handle, *const FinalizeInviteOptions) -> EResult;
}

/// The socket both channels use: EOS_P2P_SocketId "mvs".
pub fn socket_id() -> SocketId {
    let mut socket_name = [0; SOCKET_NAME_BYTES];
    for (d, s) in socket_name.iter_mut().zip(b"mvs") {
        *d = c_char::try_from(*s).unwrap_or(0);
    }
    SocketId { api_version: 1, socket_name }
}
