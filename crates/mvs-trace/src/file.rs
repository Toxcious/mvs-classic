//! A trace file: one match, every tick. Little-endian:
//!   `MVTRACE1`, u32 header length, the header (UTF-8 JSON, `Header`), then one zstd stream to the end of the file
//!   that holds per tick a u32 frame length and the frame (`Frame`'s fields in order, `field.rs`).
//! A frame is compared first by its bytes (equal bytes, equal records); only a frame whose bytes differ is read.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::TraceError;
use crate::field::{Cursor, Field};
use crate::record::{Frame, frame_schema};

pub const MAGIC: &[u8; 8] = b"MVTRACE1";
/// The format's version: the header's `format`.
pub const FORMAT: u32 = 1;

/// What a trace was run on and how to replay it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub format: u32,
    /// The match's name (from the oracle's match list).
    pub name: String,
    /// The match as that file defines it: the `World` seed, the set-up, the input source, the tick count or end.
    #[serde(rename = "match")]
    pub spec: serde_json::Value,
    /// The `MatchSetup` the World was made with (the match's, the characters' keys in place).
    pub setup: serde_json::Value,
    /// `new World(data, seed, setup)`'s seed.
    pub world_seed: f64,
    /// The stage the World's data was made with (a `StageName`); absent: the Lab's map (`trainingroomsmall`), the
    /// one stage the traces before stages had.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// The store the data came from: its build id and each kind's importer version.
    pub store: serde_json::Value,
    /// The writer's commit and run.
    pub source: serde_json::Value,
    /// The ticks written.
    pub ticks: u32,
    /// Why the run stopped: `ticks` (the count of the match), or the event that ended it.
    pub stop: String,
    /// The oracle state hash every 600 ticks, as the smoke's trace lines print it:
    /// the check that the writer only reads the sim.
    #[serde(default)]
    pub hashes: Vec<String>,
    /// The oracle state hash after the last tick (a replay of the inputs checks it).
    #[serde(default)]
    pub final_hash: Option<String>,
    /// A tutorial match's page calls, each with the index of the frame it precedes (`record::TutorialCall`); none in
    /// any other match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<(u32, crate::record::TutorialCall)>,
    /// Every leaf of a frame in write order (`record::frame_schema`).
    pub schema: Vec<String>,
}

/// A trace read whole: its header and each frame's bytes.
pub struct Trace {
    pub header: Header,
    bytes: Vec<u8>,
    /// Each frame's range in `bytes`.
    frames: Vec<(usize, usize)>,
}

impl Trace {
    pub fn read(path: &Path) -> Result<Self, TraceError> {
        let bytes = std::fs::read(path).map_err(|e| TraceError::Io(path.display().to_string(), e))?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, TraceError> {
        let mut r = Cursor::new(&bytes);
        if r.take(8)? != MAGIC {
            return Err(TraceError::Bad(0, "not a trace (no MVTRACE1)"));
        }
        let n = r.u32()? as usize;
        let header: Header = serde_json::from_slice(r.take(n)?).map_err(TraceError::Header)?;
        if header.format != FORMAT {
            return Err(TraceError::Format(header.format));
        }
        let ours = frame_schema();
        if header.schema != ours {
            let at = header
                .schema
                .iter()
                .zip(&ours)
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(header.schema.len()));
            return Err(TraceError::Schema {
                at,
                file: header.schema.get(at).cloned().unwrap_or_default(),
                ours: ours.get(at).cloned().unwrap_or_default(),
            });
        }
        let at = r.position();
        let bytes =
            zstd::decode_all(&bytes[at..]).map_err(|_| TraceError::Bad(at, "the frames are not a zstd stream"))?;
        let mut r = Cursor::new(&bytes);
        let mut frames = Vec::with_capacity(header.ticks as usize);
        while !r.done() {
            let len = r.u32()? as usize;
            let start = r.position();
            r.take(len)?;
            frames.push((start, start + len));
        }
        if frames.len() != header.ticks as usize {
            return Err(TraceError::Ticks { header: header.ticks, frames: frames.len() });
        }
        Ok(Self { header, bytes, frames })
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Frame `i`'s bytes.
    pub fn frame_bytes(&self, i: usize) -> &[u8] {
        let (a, b) = self.frames[i];
        &self.bytes[a..b]
    }

    /// Frame `i` read.
    pub fn frame(&self, i: usize) -> Result<Frame, TraceError> {
        let mut r = Cursor::new(self.frame_bytes(i));
        let f = Frame::get(&mut r)?;
        if !r.done() {
            return Err(TraceError::Bad(r.position(), "bytes left after the frame"));
        }
        Ok(f)
    }
}

/// Writes a trace: the frames as they come, the header at `finish` (its `ticks` and `schema` filled in).
pub struct TraceWriter {
    frames: Vec<u8>,
    ticks: u32,
    scratch: Vec<u8>,
}

impl Default for TraceWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceWriter {
    pub fn new() -> Self {
        Self { frames: Vec::with_capacity(1 << 20), ticks: 0, scratch: Vec::with_capacity(4096) }
    }

    pub fn push(&mut self, frame: &Frame) {
        self.scratch.clear();
        frame.put(&mut self.scratch);
        let len = u32::try_from(self.scratch.len()).expect("a frame under 4 GB");
        self.frames.extend_from_slice(&len.to_le_bytes());
        self.frames.extend_from_slice(&self.scratch);
        self.ticks += 1;
    }

    pub fn finish(self, mut header: Header) -> Vec<u8> {
        header.format = FORMAT;
        header.ticks = self.ticks;
        header.schema = frame_schema();
        let head = serde_json::to_vec(&header).expect("a header serialises");
        let frames = zstd::encode_all(self.frames.as_slice(), 3).expect("zstd into memory");
        let mut out = Vec::with_capacity(12 + head.len() + frames.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&u32::try_from(head.len()).expect("a header under 4 GB").to_le_bytes());
        out.extend_from_slice(&head);
        out.extend_from_slice(&frames);
        out
    }
}
