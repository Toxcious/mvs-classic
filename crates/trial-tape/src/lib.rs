//! Tapes: the calls a piece of the oracle's sim made in a real run, with what each returned (the oracle's
//! tape recorder writes them; the format is in `crates/README.md`). A port replays a tape's calls, compares every
//! result bit for bit, and times the replay against the oracle's replay of the same tape.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct TapeCall {
    pub id: u16,
    pub name: String,
    pub layout: String,
}

#[derive(Debug, Deserialize)]
pub struct TapeSource {
    pub commit: String,
    pub run: String,
}

#[derive(Debug, Deserialize)]
pub struct TapeHeader {
    pub slice: String,
    pub source: TapeSource,
    pub calls: Vec<TapeCall>,
    #[serde(default)]
    pub fixtures: Vec<String>,
}

impl TapeHeader {
    /// The id of the call named `name`; panics when the tape has none (a tape of another slice or version).
    pub fn id(&self, name: &str) -> u16 {
        self.calls
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("tape of slice {} has no call {name}", self.slice))
            .id
    }
}

/// A tape held whole in memory; `records()` walks it without copying.
pub struct Tape {
    pub header: TapeHeader,
    bytes: Vec<u8>,
    body: usize,
}

#[derive(Clone, Copy)]
pub struct Record<'a> {
    pub id: u16,
    pub payload: &'a [u8],
}

impl Tape {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Tape> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        Ok(Tape::from_bytes(bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Tape, String> {
        if bytes.len() < 12 || &bytes[..8] != b"MVTAPE01" {
            return Err("not a tape".into());
        }
        let head_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let header: TapeHeader =
            serde_json::from_slice(&bytes[12..12 + head_len]).map_err(|e| format!("tape header: {e}"))?;
        let tape = Tape { header, bytes, body: 12 + head_len };
        // one walk up front, so a cut tape fails here and not halfway through a timed replay
        let mut end = tape.body;
        for r in tape.records() {
            end = r.payload.as_ptr() as usize - tape.bytes.as_ptr() as usize + r.payload.len();
        }
        if end != tape.bytes.len() {
            return Err("tape ends inside a record".into());
        }
        Ok(tape)
    }

    pub fn records(&self) -> Records<'_> {
        Records { bytes: &self.bytes, at: self.body }
    }

    /// The tape's folder joined with a fixture name of its header.
    pub fn fixture_path(tape_path: impl AsRef<Path>, name: &str) -> std::path::PathBuf {
        tape_path.as_ref().parent().unwrap_or(Path::new(".")).join(name)
    }
}

pub struct Records<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Iterator for Records<'a> {
    type Item = Record<'a>;
    fn next(&mut self) -> Option<Record<'a>> {
        if self.at + 8 > self.bytes.len() {
            return None;
        }
        let b = &self.bytes[self.at..];
        let id = u16::from_le_bytes([b[0], b[1]]);
        let len = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
        if self.at + 8 + len > self.bytes.len() {
            return None;
        }
        let payload = &self.bytes[self.at + 8..self.at + 8 + len];
        self.at += 8 + len;
        Some(Record { id, payload })
    }
}

/// Reads a record's values in order (the call's `layout`).
pub struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(r: Record<'a>) -> Cursor<'a> {
        Cursor { bytes: r.payload, at: 0 }
    }
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let v: [u8; N] = self.bytes[self.at..self.at + N].try_into().unwrap();
        self.at += N;
        v
    }
    pub fn f64(&mut self) -> f64 {
        f64::from_le_bytes(self.take())
    }
    pub fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take())
    }
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }
    pub fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }
    pub fn u8(&mut self) -> u8 {
        self.take::<1>()[0]
    }
    pub fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Same bits (the sign of zero included), or both NaN: the tapes' one rule for a float. A NaN's payload is left out:
/// x86 makes a negative quiet NaN where V8 writes its canonical one, and no sim code reads a NaN's bits.
#[inline]
pub fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[inline]
pub fn same_bits32(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}
