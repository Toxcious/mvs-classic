//! The online protocol's version.

/// Bumped by hand by every change to what crosses the wire (a message's name or shape, a packet's layout). The
/// server's first message carries it and the game's hello repeats it; either side closes on another number.
/// The port's own numbers start at 23: the host core's player-facing texts travel as codes
/// (`messages`), an input carries the right stick's Y (`mvs_sim::input`, 22 bytes), a match's slots are told away
/// and back (24), a queue join carries the round trip to the relay (25), and a match's path (26): the hello's Epic
/// id, round trip and Direct setting, the path's reports and choice, the in-app referee's messages and the path pings.
/// 27: a custom lobby's mutators are a set (`lobby_rules::MutatorSet`, the option `mutators`), not one mutator.
/// 28: a custom lobby's Chaos Wheel (the option `wheel`, the set-up's `wheel`).
/// 29: a custom lobby's style is its own (`lobby_rules::LobbyStyle`: 0 Versus, 1 Free For All), no longer the
/// build's ETeamStyle.
/// 30: the Chaos Wheel option is its choices (`lobby_rules::WheelOptions`: window, warning, Chaos rate, two at once,
/// pool), and the set-up's `wheel` carries the rate and two at once.
pub const PROTOCOL: u32 = 30;

/// The origin of the app's game window (`mvs://game/`): the raw `Origin` header the server lets in by default.
pub const APP_ORIGIN: &str = "mvs://game";
