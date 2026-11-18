//! A UE 4.26 .locres file (FTextLocalizationResource::LoadFromArchive): namespace -> key -> localized string, in the
//! file's order. Versions 1 Compact, 2 Optimized_CRC32 and 3 Optimized_CityHash64_UTF16 (0 Legacy has no magic: not
//! read, the build has none). The reader of the text kind.

use indexmap::IndexMap;

use crate::archive::{ReadError, Result};

/// Namespace -> key -> string, in the file's order (a key met twice keeps its first place and its last string, as an
/// insertion-ordered map does).
pub type LocresTable = IndexMap<String, IndexMap<String, String>>;

/// The file's magic (FTextLocalizationResource's LocResMagic).
const MAGIC: [u8; 16] =
    [0x0E, 0x14, 0x74, 0x75, 0x67, 0x4A, 0x03, 0xFC, 0x4A, 0x15, 0x90, 0x9D, 0xC3, 0x37, 0x7F, 0x1B];

/// A cursor over the file with a bounds message.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
    file: &'a str,
}

impl<'a> Cursor<'a> {
    fn need(&self, n: usize) -> Result<()> {
        if self.at.checked_add(n).is_none_or(|end| end > self.b.len()) {
            return Err(ReadError::parse(format!("{}: locres ends inside a value at {}", self.file, self.at)));
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }

    /// An FString: a length with its terminator; negative = UTF-16.
    fn string(&mut self) -> Result<String> {
        let n = i64::from(self.i32()?);
        if n == 0 {
            return Ok(String::new());
        }
        if n < 0 {
            let bytes = self.take(usize::try_from(-n * 2).expect("a 32-bit length"))?;
            // the terminator is dropped, not checked
            let units: Vec<u16> =
                bytes[..bytes.len() - 2].as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
            // Deviation: a lone surrogate reads as U+FFFD (a JavaScript string keeps it); none in this build's files.
            return Ok(String::from_utf16_lossy(&units));
        }
        let bytes = self.take(usize::try_from(n).expect("a 32-bit length"))?;
        // Latin-1: each byte one character
        Ok(bytes[..bytes.len() - 1].iter().map(|&b| char::from(b)).collect())
    }
}

/// The table of a .locres file's bytes; `file` names it in an error.
pub fn read_locres(b: &[u8], file: &str) -> Result<LocresTable> {
    let mut c = Cursor { b, at: 0, file };
    let mut version = 0;
    if b.len() >= 17 && b[..16] == MAGIC {
        c.at = 16;
        version = c.u8()?;
    }
    if !(1..=3).contains(&version) {
        return Err(ReadError::parse(format!("{file}: locres version {version} is not read")));
    }
    let strings_at = c.i64()?;
    let back = c.at;
    // As in JavaScript: `at = stringsAt` then a read; an offset past the file fails the first read at that position
    c.at = usize::try_from(strings_at).unwrap_or(usize::MAX);
    let mut strings: Vec<String> = Vec::new();
    let mut n = c.i32()?;
    while n > 0 {
        strings.push(c.string()?);
        if version >= 2 {
            c.i32()?;
        }
        n -= 1;
    }
    c.at = back;
    if version >= 2 {
        c.u32()?; // the entry count
    }
    let mut out = LocresTable::new();
    for _ in 0..c.u32()? {
        if version >= 2 {
            c.u32()?;
        }
        let ns = c.string()?;
        let keys = out.entry(ns.clone()).or_default();
        for _ in 0..c.u32()? {
            if version >= 2 {
                c.u32()?;
            }
            let key = c.string()?;
            c.u32()?; // the source string's hash
            let index = c.i32()?;
            let Some(s) = usize::try_from(index).ok().and_then(|i| strings.get(i)) else {
                return Err(ReadError::parse(format!("{file}: {ns}/{key}: string index {index} of {}", strings.len())));
            };
            keys.insert(key, s.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fstring(s: &str) -> Vec<u8> {
        let mut o = (s.len() as i32 + 1).to_le_bytes().to_vec();
        o.extend_from_slice(s.as_bytes());
        o.push(0);
        o
    }

    #[test]
    fn reads_a_compact_file() {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_le_bytes()); // namespaces
        body.extend(fstring("NS"));
        body.extend_from_slice(&2u32.to_le_bytes()); // keys
        for (k, i) in [("b", 1i32), ("a", 0)] {
            body.extend(fstring(k));
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&i.to_le_bytes());
        }
        let mut f = MAGIC.to_vec();
        f.push(1);
        let strings_at = (16 + 1 + 8 + body.len()) as i64;
        f.extend_from_slice(&strings_at.to_le_bytes());
        f.extend(body);
        f.extend_from_slice(&2i32.to_le_bytes());
        f.extend(fstring("one"));
        f.extend(fstring("two"));
        let t = read_locres(&f, "x").unwrap();
        let keys: Vec<(&String, &String)> = t["NS"].iter().collect();
        assert_eq!(keys, [(&"b".to_string(), &"two".to_string()), (&"a".to_string(), &"one".to_string())]);
        assert!(read_locres(&f[..f.len() - 3], "x").is_err());
        assert!(read_locres(&[0; 8], "x").is_err());
    }
}
