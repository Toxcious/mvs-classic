//! The tapes the oracle's recorders write and the checks replay: JSON lines, one record per line.
//!
//! - [`post`]: every call the game makes on the sound engine, per sim tick (a match run): the glue's
//!   check.
//! - [`voice`]: the interpreter's output for a scripted run of posts (each voice's spec, each parameter change, the
//!   random draws, the music's decisions): the interpreter's check.
//! - [`fx`]: each effect's output on fixed input with automation: the effects' check.

pub mod fx;
pub mod post;
pub mod voice;

use std::path::Path;

use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum TapeError {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0}:{1}: {2}")]
    Line(String, usize, String),
}

/// A tape's records.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, TapeError> {
    let name = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|e| TapeError::Io(name.clone(), e))?;
    parse(&text, &name)
}

/// A tape's records from its text (`name`: for the errors).
pub fn parse<T: DeserializeOwned>(text: &str, name: &str) -> Result<Vec<T>, TapeError> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| TapeError::Line(name.to_owned(), i + 1, e.to_string())))
        .collect()
}

/// Little-endian f32 samples from standard base64 (the recorders write a Float32Array's bytes).
pub fn f32s(b64: &str) -> Result<Vec<f32>, String> {
    let bytes = base64(b64)?;
    if bytes.len() % 4 != 0 {
        return Err(format!("{} bytes is not a whole number of f32s", bytes.len()));
    }
    Ok(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

fn base64(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| -> Result<u32, String> {
        Ok(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("not base64: {:?}", c as char)),
        }
        .into())
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVsbG8h").unwrap(), b"hello!");
        // 1.0f32, -0.5f32
        assert_eq!(f32s("AACAPwAAAL8=").unwrap(), [1.0, -0.5]);
    }
}
