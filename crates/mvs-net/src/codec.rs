//! One fighter's input on the wire (and the right
//! stick's Y, the port's own): 22 bytes, little-endian. `decode(&encode(i))` equals `canon_input(i)` field by field:
//! the World steps an input only through `canon_input`, so a decoded packet steps as the device input it came from.
//!
//! The sim already holds the two functions (`mvs_sim::input::encode_input` / `decode_input`, the trace's own); this
//! module is the net layer's way to them, and where the golden vectors check them (`tests/encode_input.rs`).

use mvs_protocol::match_wire::{INPUT_BYTES, WireInput};
use mvs_sim::InputState;
use mvs_sim::input::{decode_input, encode_input};

/// `encodeInput`.
pub fn encode(i: &InputState) -> WireInput {
    let mut b = [0u8; INPUT_BYTES];
    encode_input(i, &mut b);
    b
}

/// `decodeInput`.
pub fn decode(b: &WireInput) -> InputState {
    decode_input(b)
}

/// Two inputs the World steps alike: their wire forms are equal.
pub fn same_input(a: &InputState, b: &InputState) -> bool {
    encode(a) == encode(b)
}
