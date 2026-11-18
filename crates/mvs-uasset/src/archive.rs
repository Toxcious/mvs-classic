//! The byte reader and the read errors. It follows CUE4Parse's FArchive reads.

/// A read that failed. `Parse` is what CUE4Parse's own readers throw (its ParserException); `NotPorted` is what
/// CUE4Parse reads and the port does not yet, always reported, never read around in silence.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ReadError {
    #[error("{0}")]
    Parse(String),
    #[error("not ported: {0}")]
    NotPorted(String),
    /// The paks could not give a file (an I/O error, a damaged entry).
    #[error("{0}")]
    Pak(String),
}

impl ReadError {
    pub fn parse(msg: impl Into<String>) -> ReadError {
        ReadError::Parse(msg.into())
    }
    pub fn not_ported(msg: impl Into<String>) -> ReadError {
        ReadError::NotPorted(msg.into())
    }
    /// The same kind with `prefix` put before the message.
    pub fn context(self, prefix: &str) -> ReadError {
        match self {
            ReadError::Parse(m) => ReadError::Parse(format!("{prefix}{m}")),
            ReadError::NotPorted(m) => ReadError::NotPorted(format!("{prefix}{m}")),
            ReadError::Pak(m) => ReadError::Pak(format!("{prefix}{m}")),
        }
    }
}

impl From<mvs_pak::PakError> for ReadError {
    fn from(e: mvs_pak::PakError) -> ReadError {
        ReadError::Pak(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ReadError>;

/// FGuid as four u32: A, B, C, D.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Guid(pub [u32; 4]);

impl Guid {
    /// A GUID from its four parts (for the custom-version constants: "AAAAAAAA-BBBBBBBB-..." is A, B, ...).
    pub const fn from_parts(a: u32, b: u32, c: u32, d: u32) -> Guid {
        Guid([a, b, c, d])
    }
}

/// CUE4Parse's JSON form (EGuidFormats.UniqueObjectGuid): "AAAAAAAA-BBBBBBBB-CCCCCCCC-DDDDDDDD", upper-case hex.
impl std::fmt::Display for Guid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c, d] = self.0;
        write!(f, "{a:08X}-{b:08X}-{c:08X}-{d:08X}")
    }
}

/// A little-endian reader over one buffer. Every read checks the length and fails with [`ReadError::Parse`].
#[derive(Clone)]
pub struct Archive<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
    /// For messages: the file being read.
    pub name: &'a str,
}

impl<'a> Archive<'a> {
    pub fn new(buf: &'a [u8], name: &'a str) -> Archive<'a> {
        Archive { buf, pos: 0, name }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Bytes left after `pos`.
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// Fails unless `n` bytes are left: a count checked before a loop reads that many.
    pub fn need(&self, n: usize) -> Result<()> {
        match self.pos.checked_add(n) {
            Some(end) if end <= self.buf.len() => Ok(()),
            _ => {
                Err(ReadError::Parse(format!("{}: read of {n} at {} past end {}", self.name, self.pos, self.buf.len())))
            }
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        match self.pos.checked_add(n) {
            Some(end) if end <= self.buf.len() => {
                let s = &self.buf[self.pos..end];
                self.pos = end;
                Ok(s)
            }
            _ => {
                Err(ReadError::Parse(format!("{}: read of {n} at {} past end {}", self.name, self.pos, self.buf.len())))
            }
        }
    }

    fn arr<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("take gives N bytes"))
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(i8::from_le_bytes(self.arr()?))
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.arr()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.arr()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.arr()?))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.arr()?))
    }

    /// A 32-bit bool, as FArchive::operator<<(bool&) writes it; anything but 0 or 1 is an error.
    pub fn bool(&mut self) -> Result<bool> {
        let v = self.i32()?;
        match v {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ReadError::Parse(format!("{}: invalid bool {v} at {}", self.name, self.pos - 4))),
        }
    }

    /// One byte, as ReadFlag.
    pub fn flag(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.take(n)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    /// FString: a positive length is 8-bit text with its terminating 0 (read as UTF-8, invalid bytes replaced, as
    /// a JavaScript reader's Buffer.toString does), a negative one UTF-16 with its terminating 0.
    pub fn fstring(&mut self) -> Result<String> {
        let len = self.i32()?;
        if len == 0 {
            return Ok(String::new());
        }
        if len < 0 {
            if len == i32::MIN {
                return Err(ReadError::Parse(format!("{}: bad FString length", self.name)));
            }
            let n = len.unsigned_abs() as usize * 2;
            let b = self.take(n)?;
            let units: Vec<u16> = b[..n - 2].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            return Ok(String::from_utf16_lossy(&units));
        }
        let len = len as usize;
        if len > self.remaining() {
            return Err(ReadError::Parse(format!("{}: FString length {len} at {}", self.name, self.pos - 4)));
        }
        let b = self.take(len)?;
        if b[len - 1] != 0 {
            return Err(ReadError::Parse(format!("{}: FString not null terminated", self.name)));
        }
        Ok(String::from_utf8_lossy(&b[..len - 1]).into_owned())
    }

    pub fn guid(&mut self) -> Result<Guid> {
        Ok(Guid([self.u32()?, self.u32()?, self.u32()?, self.u32()?]))
    }
}
