//! The voice tape: what the oracle's interpreter did for a scripted run (every event of a scope posted with its
//! game parameters and switches swept, then a post tape replayed), on a fake audio context in node. The
//! interpreter replayed on the same input (its random draws from the tape) must say the same, line for line.
//!
//! One line per record, `"t"` naming its kind, times on the audio clock in samples at 48 kHz:
//!
//! ```text
//! {"t":"post","tick":3,"call":{"call":"post","ev":"Play_X","owner":0}}     the input, as a post tape line
//! {"t":"random","v":0.7231}                                                a Math.random draw, in order
//! {"t":"voice","voice":7,"at":1024,"medium":123,...}                       a voice started (VoiceSpec's fields)
//! {"t":"param","at":2048,"target":{"Voice":7},"param":"Fade","value":0,"ramp":{"Linear":0.1}}   a ParamChange
//! {"t":"music","at":4096,"node":456,"what":"segment"}                      a music decision: what started
//! ```

use mvs_data::sound::Id;
use serde::{Deserialize, Serialize};

use super::post::Call;
use crate::spec::{ParamChange, VoiceSpec};

/// A line of the voice tape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Line {
    Post {
        tick: u32,
        call: Call,
    },
    Random {
        v: f64,
    },
    Voice(Box<VoiceSpec>),
    Param(ParamChange),
    /// A music decision: the node it took (a segment, a transition, a playlist item) and what it was.
    Music {
        at: u64,
        node: Id,
        what: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Param, Ramp, Target, VoiceId};

    #[test]
    fn round_trip() {
        let lines = vec![
            Line::Random { v: 0.25 },
            Line::Voice(Box::new(VoiceSpec { voice: VoiceId(7), at: 1024, medium: 123, ..VoiceSpec::default() })),
            Line::Param(ParamChange {
                at: 2048,
                target: Target::Voice(VoiceId(7)),
                param: Param::Fade,
                value: 0.0,
                ramp: Ramp::Linear(0.1),
            }),
            Line::Music { at: 4096, node: 456, what: "segment".into() },
        ];
        let text: String = lines.iter().map(|l| serde_json::to_string(l).unwrap() + "\n").collect();
        assert_eq!(crate::tape::parse::<Line>(&text, "test").unwrap(), lines);
    }
}
