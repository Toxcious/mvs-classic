//! The effects tape: each of the oracle's effect processors run in node on a fixed input with automation. The
//! port's effect run on the same input and automation must give the same samples.
//!
//! One line per case:
//!
//! ```text
//! {"case":"roomverb-impulse","fx":{"plugin":"AkRoomVerbFX",...},"er":{"left":[[ms,gain],..],"right":[..]},
//!  "rate":48000,"channels":2,"signal":"impulse at 0, then silence","frames":48000,
//!  "auto":[{"at":24000,"id":0,"value":-12}],"input":"<base64>","output":"<base64>"}
//! ```
//!
//! `meter` (a meter case) lists the values the processor posted, [frame, value]. `fx` is the plugin's parameters as the bank kind has them ([`FxParams`]); `auto` moves a parameter by the
//! plugin's id at a frame; `input` and `output` are planar little-endian f32 (`channels` × `frames`) in base64.

use mvs_data::sound::{ErTaps, FxParams};
use serde::{Deserialize, Serialize};

/// A parameter moved at a frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Auto {
    pub at: u32,
    pub id: u16,
    pub value: f64,
}

/// A case of the effects tape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub case: String,
    pub fx: FxParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub er: Option<ErTaps>,
    pub rate: u32,
    pub channels: u16,
    /// What the input is, for a reader.
    pub signal: String,
    pub frames: u32,
    #[serde(default)]
    pub auto: Vec<Auto>,
    /// A meter's posted values: [frame at the end of the block that posted, value] in order (the processor posts
    /// when its value changes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meter: Vec<[f64; 2]>,
    pub input: String,
    pub output: String,
}

impl Case {
    /// The input, planar.
    pub fn input(&self) -> Result<Vec<f32>, String> {
        super::f32s(&self.input)
    }
    /// The expected output, planar.
    pub fn output(&self) -> Result<Vec<f32>, String> {
        super::f32s(&self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::sound::GainParams;

    #[test]
    fn reads_a_case() {
        let line = r#"{"case":"gain","fx":{"plugin":"AkGainFX","fullband":-6,"lfe":0},"rate":48000,"channels":1,"signal":"two samples","frames":2,"input":"AACAPwAAAL8=","output":"AACAPwAAAL8="}"#;
        let c: Vec<Case> = crate::tape::parse(line, "test").unwrap();
        assert_eq!(c[0].fx, FxParams::AkGainFX(GainParams { fullband: -6.0, lfe: 0.0 }));
        assert_eq!(c[0].input().unwrap(), [1.0, -0.5]);
    }
}
