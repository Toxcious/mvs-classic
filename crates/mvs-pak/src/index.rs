//! One pak's footer, primary index and full directory index, read, decrypted and checked; its entry records decoded.

use std::fs::File;
use std::path::Path;

use aes::Aes256;
use aes::cipher::BlockDecrypt;
use aes::cipher::inout::InOutBuf;
use sha1::{Digest, Sha1};

use crate::{PakError, Result};

pub(crate) const MAGIC: u32 = 0x5A6F_12E1;
pub(crate) const FOOTER_SIZE: u64 = 16 + 1 + 4 + 4 + 8 + 8 + 20 + 5 * 32;
/// The mount point every pak of the build has; paths are given without it.
pub(crate) const MOUNT_PREFIX: &str = "../../../";

pub(crate) fn align16(n: u64) -> u64 {
    (n + 15) & !15
}

/// AES-256-ECB in place; `data` is whole 16-byte blocks.
pub(crate) fn aes_decrypt(cipher: &Aes256, data: &mut [u8]) -> std::result::Result<(), String> {
    if !data.len().is_multiple_of(16) {
        return Err(format!("encrypted block of {} bytes, not a multiple of 16", data.len()));
    }
    let (blocks, _) = InOutBuf::from(data).into_chunks();
    cipher.decrypt_blocks_inout(blocks);
    Ok(())
}

/// Fills `buf` from `file` at `offset` (a positioned read: it moves no cursor another reader depends on).
pub(crate) fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            let n = file.seek_read(&mut buf[done..], offset + done as u64)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("read past the end at {}", offset + done as u64),
                ));
            }
            done += n;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buf, offset)
    }
}

/// A little-endian reader over a slice; every read is bounds-checked.
pub(crate) struct Reader<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, p: 0 }
    }
    pub fn bytes(&mut self, n: usize) -> std::result::Result<&'a [u8], String> {
        let s = self
            .b
            .get(self.p..self.p.checked_add(n).ok_or("length overflows")?)
            .ok_or_else(|| format!("read of {n} bytes at {} past the end of {}", self.p, self.b.len()))?;
        self.p += n;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> std::result::Result<[u8; N], String> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }
    pub fn u8(&mut self) -> std::result::Result<u8, String> {
        Ok(self.arr::<1>()?[0])
    }
    pub fn u32(&mut self) -> std::result::Result<u32, String> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn i32(&mut self) -> std::result::Result<i32, String> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    pub fn u64(&mut self) -> std::result::Result<u64, String> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    /// A signed 64-bit size or offset; a negative one is refused.
    pub fn i64(&mut self) -> std::result::Result<u64, String> {
        let at = self.p;
        let v = i64::from_le_bytes(self.arr()?);
        u64::try_from(v).map_err(|_| format!("64-bit value {v} out of range at {at}"))
    }
    /// An FString: a negative length is UTF-16 code units, a positive one Latin-1 bytes; trailing NULs dropped.
    pub fn fstr(&mut self) -> std::result::Result<String, String> {
        let n = self.i32()?;
        let mut s = if n < 0 {
            let units = n.unsigned_abs() as usize;
            let raw = self.bytes(units.checked_mul(2).ok_or("length overflows")?)?;
            let u: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16(&u).map_err(|_| "a string that is not well-formed UTF-16".to_string())?
        } else {
            self.bytes(n as usize)?.iter().map(|&c| c as char).collect()
        };
        while s.ends_with('\0') {
            s.pop();
        }
        Ok(s)
    }
}

/// An entry as decoded from the index, with its compression blocks (start / end relative to `offset`).
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub offset: u64,
    pub size: u64,
    pub uncompressed_size: u64,
    /// 0 none, else 1-based into the footer's method names.
    pub method: u32,
    pub encrypted: bool,
    pub block_size: u64,
    /// [start, end) pairs relative to `offset`. Empty when stored.
    pub blocks: Vec<(u64, u64)>,
    /// SHA1 of the first `size` stored bytes; full records only (the encoded form drops it).
    pub sha1: Option<[u8; 20]>,
}

/// A full FPakEntry record (the index's non-encoded list, and the copy before each entry's data).
pub(crate) fn read_record(r: &mut Reader<'_>) -> std::result::Result<Record, String> {
    let offset = r.i64()?;
    let size = r.i64()?;
    let uncompressed_size = r.i64()?;
    let method = r.u32()?;
    // SHA1 of the first `size` stored bytes (still encrypted when the entry is).
    let sha1: [u8; 20] = r.bytes(20)?.try_into().unwrap();
    let mut blocks = Vec::new();
    if method != 0 {
        let n = r.i32()?;
        for _ in 0..n.max(0) {
            blocks.push((r.i64()?, r.i64()?));
        }
    }
    let flags = r.u8()?;
    let block_size = u64::from(r.u32()?);
    Ok(Record {
        offset,
        size,
        uncompressed_size,
        method,
        encrypted: flags & 1 != 0,
        block_size,
        blocks,
        sha1: Some(sha1),
    })
}

/// FPakEntry's serialized size for v11: what precedes an entry's data.
pub(crate) fn header_size(method: u32, block_count: u64) -> u64 {
    8 + 8 + 8 + 4 + 20 + 1 + 4 + if method != 0 { 4 + 16 * block_count } else { 0 }
}

/// FPakFile::DecodePakEntry of 4.26: a 32-bit word of flags and counts, then 32 or 64-bit fields.
pub(crate) fn decode_entry(b: &[u8], at: usize) -> std::result::Result<Record, String> {
    let mut r = Reader { b, p: at };
    let value = r.u32()?;
    let mut block_size = if value & 0x3f == 0x3f { u64::from(r.u32()?) } else { u64::from(value & 0x3f) << 11 };
    let method = (value >> 23) & 0x3f;
    let mut read =
        |is32: bool| -> std::result::Result<u64, String> { if is32 { Ok(u64::from(r.u32()?)) } else { r.u64() } };
    let offset = read(value & (1 << 31) != 0)?;
    let uncompressed_size = read(value & (1 << 30) != 0)?;
    let size = if method != 0 { read(value & (1 << 29) != 0)? } else { uncompressed_size };
    let encrypted = value & (1 << 22) != 0;
    let count = u64::from((value >> 6) & 0xffff);
    let mut blocks = Vec::with_capacity(count as usize);
    if count > 0 {
        if count == 1 {
            block_size = uncompressed_size;
        }
        let mut start = header_size(method, count);
        if count == 1 && !encrypted {
            blocks.push((start, start + size));
        } else {
            for _ in 0..count {
                let len = u64::from(r.u32()?);
                blocks.push((start, start + len));
                start += if encrypted { align16(len) } else { len };
            }
        }
    } else {
        block_size = 0;
    }
    Ok(Record { offset, size, uncompressed_size, method, encrypted, block_size, blocks, sha1: None })
}

/// One pak's index as read: the encoded entries, the full records, the directory index's bytes.
pub(crate) struct RawIndex {
    /// The mount point without "../../../".
    pub mount: String,
    pub methods: Vec<String>,
    pub entry_count: u32,
    pub encoded: Vec<u8>,
    /// The full records as stored, and where each starts in them.
    pub records: Vec<u8>,
    pub record_at: Vec<u32>,
    pub dir_bytes: Vec<u8>,
}

fn check_sha1(data: &[u8], want: &[u8], file: &Path, what: &str) -> Result<()> {
    if Sha1::digest(data).as_slice() != want {
        // The primary index is the one a wrong key fails first.
        if what == "index" {
            return Err(PakError::Key(file.to_path_buf()));
        }
        return Err(PakError::Format(
            file.to_path_buf(),
            format!("{what}: SHA1 mismatch (wrong key, or a damaged pak)"),
        ));
    }
    Ok(())
}

/// Reads one pak's footer, primary index and full directory index.
pub(crate) fn read_raw_index(fh: &File, file: &Path, cipher: &Aes256) -> Result<RawIndex> {
    let io = |e| PakError::Io(file.to_path_buf(), e);
    let bad = |why: String| PakError::Format(file.to_path_buf(), why);
    let file_size = fh.metadata().map_err(io)?.len();
    if file_size < FOOTER_SIZE {
        return Err(bad("too small for a pak".into()));
    }
    let mut foot = vec![0u8; FOOTER_SIZE as usize];
    read_at(fh, &mut foot, file_size - FOOTER_SIZE).map_err(io)?;
    let mut f = Reader::new(&foot);
    f.p = 16;
    let encrypted_index = f.u8().map_err(bad)? != 0;
    if f.u32().map_err(bad)? != MAGIC {
        return Err(bad("no pak footer".into()));
    }
    let version = f.i32().map_err(bad)?;
    if version != 11 {
        return Err(bad(format!("version {version}, this reader knows 11")));
    }
    let index_offset = f.i64().map_err(bad)?;
    let index_size = f.i64().map_err(bad)?;
    let index_hash = f.bytes(20).map_err(bad)?.to_vec();
    let mut methods = Vec::new();
    for _ in 0..5 {
        let raw = f.bytes(32).map_err(bad)?;
        let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        let s: String = raw[..end].iter().map(|&c| c as char).collect();
        if !s.is_empty() {
            methods.push(s);
        }
    }

    let read_dec = |off: u64, size: u64| -> Result<Vec<u8>> {
        if off.checked_add(size).is_none_or(|e| e > file_size) {
            return Err(bad(format!("index part at {off} of {size} bytes past the end")));
        }
        let mut b = vec![0u8; size as usize];
        read_at(fh, &mut b, off).map_err(io)?;
        if encrypted_index {
            aes_decrypt(cipher, &mut b).map_err(bad)?;
        }
        Ok(b)
    };
    let primary = read_dec(index_offset, index_size)?;
    check_sha1(&primary, &index_hash, file, "index")?;
    let mut r = Reader::new(&primary);
    let mount_point = r.fstr().map_err(bad)?;
    let Some(mount) = mount_point.strip_prefix(MOUNT_PREFIX) else {
        return Err(bad(format!("mount point {mount_point}, expected {MOUNT_PREFIX}...")));
    };
    let entry_count = u32::try_from(r.i32().map_err(bad)?).map_err(|_| bad("negative entry count".into()))?;
    let _path_hash_seed = r.u64().map_err(bad)?;
    if r.u32().map_err(bad)? != 0 {
        // the path hash index: offset, size, SHA1 (not read: the directory index names every path)
        r.bytes(8 + 8 + 20).map_err(bad)?;
    }
    if r.u32().map_err(bad)? == 0 {
        return Err(bad("no full directory index".into()));
    }
    let fd_off = r.i64().map_err(bad)?;
    let fd_size = r.i64().map_err(bad)?;
    let fd_hash = r.bytes(20).map_err(bad)?.to_vec();
    let n = r.i32().map_err(bad)?;
    let encoded = r.bytes(usize::try_from(n).map_err(|_| bad("negative encoded size".into()))?).map_err(bad)?.to_vec();
    let n_records = r.i32().map_err(bad)?;
    let rec_start = r.p;
    let mut record_at = Vec::with_capacity(n_records.max(0) as usize);
    for _ in 0..n_records.max(0) {
        record_at.push((r.p - rec_start) as u32);
        read_record(&mut r).map_err(bad)?;
    }
    let records = primary[rec_start..r.p].to_vec();

    let dir_bytes = read_dec(fd_off, fd_size)?;
    check_sha1(&dir_bytes, &fd_hash, file, "directory index")?;
    Ok(RawIndex { mount: mount.to_string(), methods, entry_count, encoded, records, record_at, dir_bytes })
}

/// Calls `f` per file of a pak's full directory index, in index order, with its directory (mount point joined, ''
/// for the mount's root), name and location.
pub(crate) fn each_file(ix: &RawIndex, mut f: impl FnMut(&str, String, i32)) -> std::result::Result<u64, String> {
    let mut d = Reader::new(&ix.dir_bytes);
    let mut n = 0;
    let mut dir = String::new();
    for _ in 0..d.i32()?.max(0) {
        let name = d.fstr()?;
        dir.clear();
        dir.push_str(&ix.mount);
        if name != "/" {
            dir.push_str(&name);
        }
        for _ in 0..d.i32()?.max(0) {
            let file = d.fstr()?;
            let loc = d.i32()?;
            f(&dir, file, loc);
            n += 1;
        }
    }
    Ok(n)
}

/// The record at an index location: >= 0 decoded from the encoded entries, < 0 a full record.
pub(crate) fn entry_at(
    encoded: &[u8],
    records: &[u8],
    record_at: &[u32],
    loc: i32,
) -> std::result::Result<Record, String> {
    if loc >= 0 {
        return decode_entry(encoded, loc as usize);
    }
    let i = (-(i64::from(loc)) - 1) as usize;
    let at = *record_at.get(i).ok_or_else(|| format!("bad entry location {loc}"))?;
    let mut r = Reader { b: records, p: at as usize };
    read_record(&mut r)
}
