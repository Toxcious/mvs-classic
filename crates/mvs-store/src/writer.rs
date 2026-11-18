//! The pack writer: one kind's entries into one `MVSPACK2` file (the format in this crate's root). An entry is
//! compressed (zstd level 3, one frame) unless that saves less than 4 KB or an eighth, or its content type is
//! compressed already; the index is sorted by path; the pack is written to `<file>.tmp`, flushed, reopened and checked
//! (entry count, every entry inside the data), then renamed into place.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::{CODEC_RAW, CODEC_ZSTD, FOOT, FOOTER, HEAD, Pack, Result, StoreError};

/// zstd's level for an entry.
const LEVEL: i32 = 3;
/// Content types whose bytes are compressed already: stored as they are, not tried (Ogg 1.01x, PNG 1.10x).
const STORED_TYPES: [&str; 2] = ["audio/ogg", "image/png"];
/// An entry that zstd shrinks by less than this is stored as it is (one under it, untried): each decode costs the
/// reader more than 4 KB of disk is worth.
const MIN_SAVED: usize = 4096;

/// An entry as the pack stores it: its stored bytes and its codec ([`crate::CODEC_RAW`] or zstd).
pub fn encode_entry<'a>(bytes: &'a [u8], content_type: &str) -> Result<(Cow<'a, [u8]>, u8)> {
    let size = bytes.len();
    if size < MIN_SAVED || STORED_TYPES.contains(&content_type) {
        return Ok((Cow::Borrowed(bytes), CODEC_RAW));
    }
    let data = zstd::bulk::compress(bytes, LEVEL)
        .map_err(|e| StoreError::Format(PathBuf::from("zstd"), format!("compressing {size} bytes: {e}")))?;
    if size.saturating_sub(data.len()) < MIN_SAVED.max(size >> 3) {
        return Ok((Cow::Borrowed(bytes), CODEC_RAW));
    }
    Ok((Cow::Owned(data), CODEC_ZSTD))
}

/// What [`PackWriter::finish`] wrote.
#[derive(Clone, Debug)]
pub struct PackSummary {
    pub path: PathBuf,
    pub entries: u64,
    /// The pack file's bytes.
    pub bytes: u64,
}

#[derive(Clone, Copy)]
struct Record {
    offset: u64,
    length: u32,
    size: u32,
    content_type: u8,
    codec: u8,
}

/// Writes one pack. Entries may be added in any order (from one thread; a runner on several threads sends them to
/// the one writer); the index is sorted at the end. A path added twice keeps the first.
pub struct PackWriter {
    path: PathBuf,
    tmp: PathBuf,
    /// `None` once finished.
    out: Option<BufWriter<File>>,
    /// Where the next entry's bytes go.
    end: u64,
    index: HashMap<String, Record>,
    types: Vec<String>,
}

/// Ordinal order over UTF-16 code units (JavaScript's sort order). UTF-8 byte order is the same
/// except where a character above U+FFFF meets one in U+E000..U+FFFF.
fn cmp_utf16(a: &str, b: &str) -> std::cmp::Ordering {
    let astral = |s: &str| s.bytes().any(|c| c >= 0xF0);
    if astral(a) || astral(b) { a.encode_utf16().cmp(b.encode_utf16()) } else { a.cmp(b) }
}

impl PackWriter {
    /// Starts the pack at `path` (written to `<path>.tmp` until [`PackWriter::finish`]).
    pub fn create(path: &Path) -> Result<PackWriter> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let file = File::create(&tmp).map_err(|e| StoreError::Io(tmp.clone(), e))?;
        let mut out = BufWriter::with_capacity(1 << 20, file);
        out.write_all(HEAD).map_err(|e| StoreError::Io(tmp.clone(), e))?;
        Ok(PackWriter {
            path: path.to_path_buf(),
            tmp,
            out: Some(out),
            end: HEAD.len() as u64,
            index: HashMap::new(),
            types: Vec::new(),
        })
    }

    /// The entries added so far.
    pub fn entries(&self) -> usize {
        self.index.len()
    }

    /// The stored bytes of the entries so far (what they take on disk).
    pub fn bytes(&self) -> u64 {
        self.end - HEAD.len() as u64
    }

    /// Adds an entry: its path, its content type (`application/json`, ...) and its own bytes, compressed here.
    pub fn add(&mut self, path: &str, content_type: &str, bytes: &[u8]) -> Result<()> {
        let (data, codec) = encode_entry(bytes, content_type)?;
        let size = u32::try_from(bytes.len())
            .map_err(|_| StoreError::Format(self.tmp.clone(), format!("{path}: an entry of {} bytes", bytes.len())))?;
        self.add_stored(path, content_type, &data, codec, size)
    }

    /// Adds an entry already as the pack stores it ([`encode_entry`] run elsewhere, on the thread that built it):
    /// its path, content type, stored bytes, codec and own size.
    pub fn add_stored(&mut self, path: &str, content_type: &str, data: &[u8], codec: u8, size: u32) -> Result<()> {
        let tmp = self.tmp.clone();
        let bad = |why: String| StoreError::Format(tmp.clone(), why);
        // a path given twice keeps the first; the second is not written
        if self.index.contains_key(path) {
            eprintln!("pack {}: {path} added twice, the first kept", self.path.display());
            return Ok(());
        }
        if path.len() > usize::from(u16::MAX) {
            return Err(bad(format!("{path}: a path of {} bytes", path.len())));
        }
        if codec != CODEC_RAW && codec != CODEC_ZSTD {
            return Err(bad(format!("{path}: codec {codec}")));
        }
        let content_type = self.type_of(content_type)?;
        let out = self.out.as_mut().ok_or_else(|| bad("the pack is finished".into()))?;
        out.write_all(data).map_err(|e| StoreError::Io(self.tmp.clone(), e))?;
        let offset = self.end;
        self.end += data.len() as u64;
        self.index.insert(path.to_string(), Record { offset, length: data.len() as u32, size, content_type, codec });
        Ok(())
    }

    fn type_of(&mut self, content_type: &str) -> Result<u8> {
        if let Some(i) = self.types.iter().position(|t| t == content_type) {
            return Ok(i as u8);
        }
        if self.types.len() >= 255 {
            return Err(StoreError::Format(self.tmp.clone(), "more than 255 content types in one pack".into()));
        }
        if content_type.len() > usize::from(u16::MAX) {
            return Err(StoreError::Format(self.tmp.clone(), "a content type too long".into()));
        }
        self.types.push(content_type.to_string());
        Ok((self.types.len() - 1) as u8)
    }

    /// Writes the index and the footer, checks the file and renames it into place.
    pub fn finish(mut self) -> Result<PackSummary> {
        let io = |e| StoreError::Io(self.tmp.clone(), e);
        let mut paths: Vec<&String> = self.index.keys().collect();
        paths.sort_unstable_by(|a, b| cmp_utf16(a, b));
        let mut ix = Vec::with_capacity(8 + paths.iter().map(|p| p.len() + 20).sum::<usize>());
        ix.extend_from_slice(&(paths.len() as u32).to_le_bytes());
        ix.extend_from_slice(&(self.types.len() as u32).to_le_bytes());
        for t in &self.types {
            ix.extend_from_slice(&(t.len() as u16).to_le_bytes());
            ix.extend_from_slice(t.as_bytes());
        }
        for p in &paths {
            let e = &self.index[*p];
            ix.extend_from_slice(&(e.offset as f64).to_le_bytes());
            ix.extend_from_slice(&e.length.to_le_bytes());
            ix.extend_from_slice(&e.size.to_le_bytes());
            ix.push(e.content_type);
            ix.push(e.codec);
            ix.extend_from_slice(&(p.len() as u16).to_le_bytes());
            ix.extend_from_slice(p.as_bytes());
        }
        let index_offset = self.end;
        let index_len = u32::try_from(ix.len())
            .map_err(|_| StoreError::Format(self.tmp.clone(), format!("an index of {} bytes", ix.len())))?;
        let mut footer = [0u8; FOOTER as usize];
        footer[0..8].copy_from_slice(&(index_offset as f64).to_le_bytes());
        footer[8..12].copy_from_slice(&index_len.to_le_bytes());
        footer[16..24].copy_from_slice(FOOT);
        let mut out = self.out.take().ok_or_else(|| StoreError::Format(self.tmp.clone(), "finished twice".into()))?;
        out.write_all(&ix).map_err(io)?;
        out.write_all(&footer).map_err(io)?;
        let file = out.into_inner().map_err(|e| io(e.into_error()))?;
        file.sync_all().map_err(io)?;
        drop(file);
        let bytes = index_offset + ix.len() as u64 + FOOTER;
        let entries = paths.len() as u64;
        let switched = verify_pack(&self.tmp, entries, index_offset)
            .and_then(|()| crate::manage::rename_retry(&self.tmp, &self.path));
        if let Err(e) = switched {
            let _ = std::fs::remove_file(&self.tmp);
            return Err(e);
        }
        Ok(PackSummary { path: self.path.clone(), entries, bytes })
    }
}

impl Drop for PackWriter {
    /// A writer dropped unfinished (a failed or aborted run) removes its temp file.
    fn drop(&mut self) {
        if self.out.take().is_some() {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

/// Reopens a finished pack and checks it before it is switched in: it opens, holds `entries` entries, and every entry
/// lies between the head and the index, none overlapping another.
fn verify_pack(file: &Path, entries: u64, index_offset: u64) -> Result<()> {
    let bad = |why: String| StoreError::Format(file.to_path_buf(), why);
    let pack = Pack::open("verify", file)?;
    if pack.len() as u64 != entries {
        return Err(bad(format!("holds {} entries, {entries} were written", pack.len())));
    }
    let mut spans: Vec<(u64, u64, &str)> = Vec::with_capacity(pack.len());
    for p in pack.paths() {
        let e = pack.record(p).unwrap();
        let len = u64::from(e.length);
        if e.offset < HEAD.len() as u64 || e.offset + len > index_offset {
            return Err(bad(format!("{p} lies outside the data ({} + {len})", e.offset)));
        }
        // an empty entry holds no bytes to overlap
        if len > 0 {
            spans.push((e.offset, len, p));
        }
    }
    spans.sort_unstable_by_key(|s| s.0);
    for w in spans.windows(2) {
        if w[0].0 + w[0].1 > w[1].0 {
            return Err(bad(format!("{} and {} overlap ({} + {}, {})", w[0].2, w[1].2, w[0].0, w[0].1, w[1].0)));
        }
    }
    Ok(())
}
