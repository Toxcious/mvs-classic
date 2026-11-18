//! The online protocol's version.

/// Bumped by hand by every change to what crosses the wire (a message's name or shape, a packet's layout). The
/// server's first message carries it and the game's hello repeats it; either side closes on another number.
/// The port's own numbers start at 23: the host core's player-facing texts travel as codes
/// (`messages`), an input carries the right stick's Y (`mvs_sim::input`, 22 bytes), a match's slots are told away
/// and back (24), a queue join carries the round trip to the relay (25), and a match's path (26): the hello's Epic
/// id, round trip and Direct setting, the path's reports and choice, the in-app referee's messages and the path pings.
pub const PROTOCOL: u32 = 26;

/// The origin of the app's game window (`mvs://game/`): the raw `Origin` header the server lets in by default.
pub const APP_ORIGIN: &str = "mvs://game";
