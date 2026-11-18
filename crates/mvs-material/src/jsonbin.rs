//! The store's container for an entry whose header is structured: a 4-byte magic, u32 header
//! length, the header as UTF-8 JSON, padding to 16, then the body, every section named in the header by its offset
//! from the body's start.

use serde::Deserialize;

use crate::{Error, Result};

/// A section of the body: offset from the body's start and length in bytes.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct BodyRef {
    pub offset: usize,
    pub length: usize,
}

/// An entry split into its header's JSON and its body.
pub struct JsonBin<'a> {
    pub header: &'a [u8],
    pub body: &'a [u8],
}

const fn align16(n: usize) -> usize {
    (n + 15) & !15
}

impl<'a> JsonBin<'a> {
    pub fn split(magic: &[u8; 4], bytes: &'a [u8], what: &str) -> Result<JsonBin<'a>> {
        if bytes.len() < 8 || &bytes[..4] != magic {
            return Err(Error::format(what, format!("not an {} entry", String::from_utf8_lossy(magic))));
        }
        let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let body_at = align16(8 + len);
        if bytes.len() < 8 + len {
            return Err(Error::format(what, format!("a {len}-byte header in {} bytes", bytes.len())));
        }
        Ok(JsonBin { header: &bytes[8..8 + len], body: bytes.get(body_at..).unwrap_or(&[]) })
    }

    /// The header parsed into `T`.
    pub fn parse<T: for<'de> Deserialize<'de>>(&self, what: &str) -> Result<T> {
        serde_json::from_slice(self.header).map_err(|e| Error::format(what, format!("header: {e}")))
    }

    /// A section's bytes.
    pub fn section(&self, r: BodyRef, what: &str) -> Result<&'a [u8]> {
        self.body.get(r.offset..r.offset + r.length).ok_or_else(|| {
            Error::format(what, format!("section {}+{} past the body's {}", r.offset, r.length, self.body.len()))
        })
    }
}
