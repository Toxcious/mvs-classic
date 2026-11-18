//! The beta's paks as the import reads them: UE 4.26 pak v11 (zlib only, the index AES-256-ECB), several paks merged
//! in priority order, paths looked up without regard to case as UE does.
//!
//! Layout (each point checked against the build's paks by the tests):
//! - Footer, the last 221 bytes: key guid (16), encrypted-index flag (u8), magic 0x5A6F12E1, version (11), index
//!   offset (i64), index size (i64), index SHA1 (20), then 5 compression method names of 32 bytes each.
//! - Primary index (AES-256-ECB when flagged; its SHA1 over the decrypted bytes): mount point, entry count, path hash
//!   seed, the path hash index and the full directory index (each elsewhere in the file and encrypted alike), the
//!   encoded entries and the entries too large to encode.
//! - An entry's data starts with a copy of its FPakEntry record; compression blocks are offsets relative to the
//!   entry's offset, each block zlib, aligned to 16 bytes when the entry is encrypted.
//!
//! [`PakSet`] reads from any number of threads at once (`&self`, positioned reads).

mod index;
pub mod paths;
pub mod setup;

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use aes::Aes256;
use aes::cipher::KeyInit;
use flate2::{Decompress, FlushDecompress, Status};
use sha1::{Digest, Sha1};

use index::{align16, header_size, read_at};
pub use paths::BuildPaths;

#[derive(Debug, thiserror::Error)]
pub enum PakError {
    #[error("pak {0}: {1}")]
    Io(PathBuf, std::io::Error),
    /// A pak whose bytes are not what the reader expects (bad magic, version, a block past the entry).
    #[error("pak {0}: {1}")]
    Format(PathBuf, String),
    /// The key does not open an index (its SHA1 does not match the decrypted bytes).
    #[error("pak {0}: the key does not open its index")]
    Key(PathBuf),
    #[error("pak: no entry {0}")]
    NoEntry(String),
    /// Setup's check of the player's folder (a missing pak, a size or hash that is not the build's).
    #[error("setup: {0}")]
    Setup(String),
    /// A hash pass ended by its cancel flag ([`setup::setup_check`]).
    #[error("setup: cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, PakError>;

/// The paks' AES-256 key. It comes from the player's own exe (`setup::key_from_exe`), never from a constant.
#[derive(Clone)]
pub struct AesKey(pub [u8; 32]);

impl std::fmt::Debug for AesKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AesKey(..)")
    }
}

/// One path of the merged index.
#[derive(Clone, Debug)]
pub struct PakEntry {
    /// Full path with the pak's mount point joined, as the paks spell it: `MultiVersus/Content/A/B.uasset`.
    pub path: String,
    /// The pak it is read from (the highest-priority pak that has it): its place in [`PakSet::pak_files`].
    pub pak: usize,
    pub offset: u64,
    pub size: u64,
    pub uncompressed_size: u64,
    pub compressed: bool,
    pub encrypted: bool,
}

/// One mounted pak: its file handles (positioned reads) and its index's records.
struct Pak {
    name: String,
    /// One handle per core: Windows serializes the reads of one handle opened for synchronous I/O, so threads that
    /// share one wait on each other; each thread reads through the handle of its slot ([`thread_slot`]).
    handles: Vec<File>,
    methods: Vec<String>,
    encoded: Vec<u8>,
    records: Vec<u8>,
    record_at: Vec<u32>,
}

/// The paks of a folder, merged: a later (higher-priority) pak's entry wins.
pub struct PakSet {
    files: Vec<PathBuf>,
    paks: Vec<Pak>,
    cipher: Aes256,
    /// Sorted by path, ordinal over UTF-16 code units.
    entries: Vec<PakEntry>,
    /// Per entry, its location in its pak's index.
    locs: Vec<i32>,
    /// Per entry, the hash of its lower-cased path.
    hashes: Vec<u64>,
    /// An open-addressed table over `hashes`: a power of two of slots, each an entry or `EMPTY`.
    slots: Vec<u32>,
}

const EMPTY: u32 = u32::MAX;

/// File handles opened per pak: one per core, at most 64.
fn handles_per_pak() -> usize {
    std::thread::available_parallelism().map_or(8, |n| n.get()).clamp(1, 64)
}

/// This thread's slot among a pak's handles: threads take slots in turn as they first read.
fn thread_slot() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static SLOT: usize = NEXT.fetch_add(1, AtomicOrdering::Relaxed);
    }
    SLOT.with(|s| *s)
}

impl Pak {
    /// The handle this thread reads through.
    fn file(&self) -> &File {
        &self.handles[thread_slot() % self.handles.len()]
    }
}

/// The most stored bytes an entry read holds at once: a compressed entry's blocks (64 KB each in this build) are read
/// in runs of up to this many, a stored entry's data in chunks of it. A multiple of 16 (a stored entry's chunks are
/// deciphered one by one).
const READ_CHUNK: u64 = 1 << 20;

/// FNV-1a as a std hasher (the per-pak set of paths as written).
#[derive(Default)]
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

/// One path of the merge: as written, the hash of it lower-cased, its pak and its location in that pak's index.
struct Merged {
    path: String,
    hash: u64,
    pak: usize,
    loc: i32,
}

/// FNV-1a over a path's bytes lower-cased (String::to_lowercase, JS's toLowerCase): the lookup table's hash.
fn lower_hash(path: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut step = |b: u8| h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    if path.is_ascii() {
        path.bytes().for_each(|b| step(b.to_ascii_lowercase()));
    } else {
        path.to_lowercase().bytes().for_each(step);
    }
    h
}

fn has_astral(s: &str) -> bool {
    s.bytes().any(|c| c >= 0xF0)
}

/// Ordinal order over UTF-16 code units (JavaScript's string order). UTF-8 byte order is the
/// same except where a character above U+FFFF meets one in U+E000..U+FFFF.
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    if has_astral(a) || has_astral(b) { a.encode_utf16().cmp(b.encode_utf16()) } else { a.cmp(b) }
}

/// Whether two paths are equal lower-cased.
fn same_lower(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() { a.eq_ignore_ascii_case(b) } else { a.to_lowercase() == b.to_lowercase() }
}

impl PakSet {
    /// Opens `files` (lowest priority first, [`pak_files_in`]'s order) with `key`; fails on a pak the key does not
    /// open. A pak the build list names must have the size it lists.
    ///
    /// Unverified: the later-wins order is not decoded from the engine (how 4.26 orders paks of equal priority); no
    /// path of this build is in two paks, and setup refuses extra paks until the order is decoded.
    pub fn open(files: &[PathBuf], key: &AesKey) -> Result<PakSet> {
        let cipher = Aes256::new(&key.0.into());
        let sizes = setup::build_sizes();
        let mut paks = Vec::with_capacity(files.len());
        let mut raws = Vec::with_capacity(files.len());
        if files.len() > 255 {
            return Err(PakError::Format(PathBuf::new(), format!("{} paks, the set holds at most 255", files.len())));
        }
        for path in files {
            let io = |e| PakError::Io(path.clone(), e);
            let file = File::open(path).map_err(io)?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(&want) = sizes.get(&name.to_lowercase()) {
                let size = file.metadata().map_err(io)?.len();
                if size != want {
                    return Err(PakError::Format(path.clone(), format!("{size} bytes, the build list lists {want}")));
                }
            }
            let raw = index::read_raw_index(&file, path, &cipher)?;
            let mut handles = vec![file];
            for _ in 1..handles_per_pak() {
                handles.push(File::open(path).map_err(io)?);
            }
            paks.push(Pak {
                name,
                handles,
                methods: raw.methods.clone(),
                encoded: Vec::new(),
                records: Vec::new(),
                record_at: Vec::new(),
            });
            raws.push(raw);
        }

        // lower-cased path -> entry, through an open-addressed table on its hash; a later pak's entry (and its path
        // as written) replaces an earlier one's
        let total: u64 = raws.iter().map(|r| u64::from(r.entry_count)).sum();
        let mut n_slots = 1usize;
        while (n_slots as u64) < 2 * total.max(1) {
            n_slots *= 2;
        }
        let mask = n_slots - 1;
        let mut slots = vec![EMPTY; n_slots];
        let mut merged: Vec<Merged> = Vec::with_capacity(total as usize);
        for (p, raw) in raws.iter().enumerate() {
            let bad = |why: String| PakError::Format(files[p].clone(), why);
            let mut list: Vec<(String, i32)> = Vec::with_capacity(raw.entry_count as usize);
            index::each_file(raw, |dir, name, loc| {
                let mut path = String::with_capacity(dir.len() + name.len());
                path.push_str(dir);
                path.push_str(&name);
                list.push((path, loc));
            })
            .map_err(bad)?;
            // the index's count is of distinct paths as written; checked before the merge, so the
            // merge table (twice the listed counts) never fills
            let distinct = {
                let mut seen: HashSet<&str, BuildHasherDefault<Fnv>> = HashSet::default();
                seen.extend(list.iter().map(|(path, _)| path.as_str()));
                seen.len()
            };
            if distinct as u64 != u64::from(raw.entry_count) {
                return Err(bad(format!("{distinct} paths for {} entries", raw.entry_count)));
            }
            for (path, loc) in list {
                let hash = lower_hash(&path);
                let mut s = hash as usize & mask;
                loop {
                    let e = slots[s];
                    if e == EMPTY {
                        slots[s] = merged.len() as u32;
                        merged.push(Merged { path, hash, pak: p, loc });
                        break;
                    }
                    let m = &mut merged[e as usize];
                    if m.hash == hash && same_lower(&m.path, &path) {
                        *m = Merged { path, hash, pak: p, loc };
                        break;
                    }
                    s = (s + 1) & mask;
                }
            }
        }
        drop(slots);
        if merged.iter().any(|m| has_astral(&m.path)) {
            merged.sort_unstable_by(|a, b| cmp_utf16(&a.path, &b.path));
        } else {
            merged.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        }

        let mut entries = Vec::with_capacity(merged.len());
        let mut locs = Vec::with_capacity(merged.len());
        let mut hashes = Vec::with_capacity(merged.len());
        for Merged { path, hash, pak: p, loc } in merged {
            let raw = &raws[p];
            let rec = index::entry_at(&raw.encoded, &raw.records, &raw.record_at, loc)
                .map_err(|e| PakError::Format(files[p].clone(), format!("{path}: {e}")))?;
            hashes.push(hash);
            entries.push(PakEntry {
                path,
                pak: p,
                offset: rec.offset,
                size: rec.size,
                uncompressed_size: rec.uncompressed_size,
                compressed: rec.method != 0,
                encrypted: rec.encrypted,
            });
            locs.push(loc);
        }
        let mut slots = vec![EMPTY; n_slots];
        for (i, &h) in hashes.iter().enumerate() {
            let mut s = h as usize & mask;
            while slots[s] != EMPTY {
                s = (s + 1) & mask;
            }
            slots[s] = i as u32;
        }
        for (pak, raw) in paks.iter_mut().zip(raws) {
            pak.encoded = raw.encoded;
            pak.records = raw.records;
            pak.record_at = raw.record_at;
        }
        Ok(PakSet { files: files.to_vec(), paks, cipher, entries, locs, hashes, slots })
    }

    /// The pak files, in the order given to [`PakSet::open`].
    pub fn pak_files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Every path once, sorted ordinal (UTF-16 code units, as JavaScript lists them).
    pub fn paths(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|e| e.path.as_str())
    }

    /// Every entry, in [`PakSet::paths`]'s order.
    pub fn entries(&self) -> &[PakEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn find(&self, path: &str) -> Option<usize> {
        let h = lower_hash(path);
        let mask = self.slots.len() - 1;
        let mut s = h as usize & mask;
        loop {
            let i = self.slots[s];
            if i == EMPTY {
                return None;
            }
            let i = i as usize;
            if self.hashes[i] == h && same_lower(&self.entries[i].path, path) {
                return Some(i);
            }
            s = (s + 1) & mask;
        }
    }

    /// The entry of `path`, looked up without regard to case.
    pub fn entry(&self, path: &str) -> Option<&PakEntry> {
        self.find(path).map(|i| &self.entries[i])
    }

    /// The entry's bytes, decrypted and decompressed.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let i = self.find(path).ok_or_else(|| PakError::NoEntry(path.to_string()))?;
        let size = self.entries[i].uncompressed_size;
        self.read_entry(i, 0, size)
    }

    /// `len` bytes of the entry from `offset`, reading and inflating only the blocks they lie in.
    pub fn read_range(&self, path: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let i = self.find(path).ok_or_else(|| PakError::NoEntry(path.to_string()))?;
        let e = &self.entries[i];
        if offset.checked_add(len).is_none_or(|end| end > e.uncompressed_size) {
            return Err(self.bad(i, format!("bytes {offset} + {len} lie outside its {}", e.uncompressed_size)));
        }
        self.read_entry(i, offset, offset + len)
    }

    fn bad(&self, i: usize, why: String) -> PakError {
        let e = &self.entries[i];
        PakError::Format(self.files[e.pak].clone(), format!("{}: {why}", e.path))
    }

    /// Bytes [from, to) of entry `i`, decrypted and decompressed. A compressed entry reads, deciphers and inflates
    /// only the blocks the range lies in; a stored one reads all its bytes for the SHA1 its header holds (it has no
    /// zlib check of its own) and keeps the range.
    fn read_entry(&self, i: usize, from: u64, to: u64) -> Result<Vec<u8>> {
        let e = &self.entries[i];
        let pak = &self.paks[e.pak];
        let rec =
            index::entry_at(&pak.encoded, &pak.records, &pak.record_at, self.locs[i]).map_err(|w| self.bad(i, w))?;
        if rec.method != 0 {
            let m = pak.methods.get(rec.method as usize - 1);
            if !m.is_some_and(|m| m.eq_ignore_ascii_case("zlib")) {
                let m = m.cloned().unwrap_or_else(|| rec.method.to_string());
                return Err(PakError::Format(
                    self.files[e.pak].clone(),
                    format!("{}: compression {m} not supported", pak.name),
                ));
            }
        }
        let io = |err| PakError::Io(self.files[e.pak].clone(), err);
        let mut out = vec![0u8; (to - from) as usize];
        if rec.method == 0 {
            // the entry's own header precedes its data: read with the first chunk
            let hs = header_size(0, 0);
            let span = if rec.encrypted { align16(rec.size) } else { rec.size };
            let mut buf = vec![0u8; (hs + span.min(READ_CHUNK)) as usize];
            let mut sha = Sha1::new();
            let mut want: Option<[u8; 20]> = None;
            let damaged = || self.bad(i, "stored bytes do not match the entry's SHA1 (a damaged pak)".into());
            let mut at = 0u64;
            loop {
                let n = READ_CHUNK.min(span - at);
                let data: &mut [u8] = if at == 0 {
                    let b = &mut buf[..(hs + n) as usize];
                    read_at(pak.file(), b, rec.offset).map_err(io)?;
                    let hdr =
                        index::read_record(&mut index::Reader::new(&b[..hs as usize])).map_err(|w| self.bad(i, w))?;
                    if hdr.size != rec.size {
                        return Err(damaged());
                    }
                    want = hdr.sha1;
                    &mut b[hs as usize..]
                } else {
                    let b = &mut buf[..n as usize];
                    read_at(pak.file(), b, rec.offset + hs + at).map_err(io)?;
                    b
                };
                sha.update(&data[..rec.size.saturating_sub(at).min(n) as usize]);
                if from.max(at) < to.min(at + n) {
                    if rec.encrypted {
                        index::aes_decrypt(&self.cipher, data).map_err(|w| self.bad(i, w))?;
                    }
                    keep(&mut out, from, to, data, at);
                }
                at += n;
                if at >= span {
                    break;
                }
            }
            if want.is_none_or(|w| sha.finalize().as_slice() != w) {
                return Err(damaged());
            }
            return Ok(out);
        }
        let bl = &rec.blocks;
        let size = rec.uncompressed_size;
        if bl.is_empty() {
            if size == 0 {
                return Ok(out);
            }
            return Err(self.bad(i, format!("compressed entry of {size} bytes has no blocks")));
        }
        let bs = rec.block_size;
        if bs == 0 || size.div_ceil(bs) != bl.len() as u64 {
            return Err(self.bad(i, format!("{} blocks of {bs} bytes for {size} bytes", bl.len())));
        }
        if from == to {
            return Ok(out);
        }
        // a block's stored extent: an encrypted one is padded to 16 bytes
        let end = |k: usize| {
            let (s, e) = bl[k];
            s + if rec.encrypted { align16(e - s) } else { e - s }
        };
        let last = ((to - 1) / bs) as usize;
        let mut buf: Vec<u8> = Vec::new();
        let mut scratch: Vec<u8> = Vec::new();
        let mut z = Decompress::new(true);
        let mut first = (from / bs) as usize;
        while first <= last {
            let start = bl[first].0;
            let mut j = first;
            while j < last && end(j + 1) - start <= READ_CHUNK {
                j += 1;
            }
            let n = (end(j) - start) as usize;
            if buf.len() < n {
                buf.resize(n.max(READ_CHUNK.min(end(last) - start) as usize), 0);
            }
            let raw = &mut buf[..n];
            read_at(pak.file(), raw, rec.offset + start).map_err(io)?;
            // ECB deciphers each 16 bytes alone: a run whose blocks all start at 16-byte steps from its first (every
            // entry of this build) is deciphered in one call
            let whole = rec.encrypted && (first + 1..=j).all(|k| (bl[k].0 - start) % 16 == 0);
            if whole {
                index::aes_decrypt(&self.cipher, raw).map_err(|w| self.bad(i, w))?;
            }
            for (k, &(b0, b1)) in bl.iter().enumerate().take(j + 1).skip(first) {
                let s = (b0 - start) as usize;
                let len = (b1 - b0) as usize;
                let block: &[u8] = if rec.encrypted && !whole {
                    let b = &mut raw[s..s + align16(len as u64) as usize];
                    index::aes_decrypt(&self.cipher, b).map_err(|w| self.bad(i, w))?;
                    &b[..len]
                } else {
                    &raw[s..s + len]
                };
                let at = k as u64 * bs;
                let want = bs.min(size - at);
                // a block that lies whole in the range is inflated straight into the output
                let direct = at >= from && at + want <= to;
                let dst: &mut [u8] = if direct {
                    &mut out[(at - from) as usize..(at - from + want) as usize]
                } else {
                    scratch.resize(want as usize, 0);
                    &mut scratch[..]
                };
                z.reset(true);
                let status = z.decompress(block, dst, FlushDecompress::Finish);
                if !matches!(status, Ok(Status::StreamEnd)) || z.total_out() != want {
                    let why = match status {
                        Err(e) => format!("block {k}: zlib: {e}"),
                        Ok(_) => format!("block {k} inflated to {} or more, expected {want}", z.total_out()),
                    };
                    return Err(self.bad(i, why));
                }
                if !direct {
                    keep(&mut out, from, to, &scratch, at);
                }
            }
            first = j + 1;
        }
        Ok(out)
    }
}

/// Copies the part of `plain` (an entry's bytes from `at`) that lies in [from, to) into `out` (bytes from `from`).
fn keep(out: &mut [u8], from: u64, to: u64, plain: &[u8], at: u64) {
    let lo = from.max(at);
    let hi = to.min(at + plain.len() as u64);
    if lo < hi {
        out[(lo - from) as usize..(hi - from) as usize].copy_from_slice(&plain[(lo - at) as usize..(hi - at) as usize]);
    }
}

/// The build's paks in `dir` (`pakchunk<n>-WindowsNoEditor.pak`), lowest priority first, as UE mounts them: chunk
/// order, a `_P` patch pak after the others (UE mounts patch paks above the base ones).
///
/// Unverified: the order among base chunks matters only where two hold one path; no path of this build is in two
/// paks, and the later-wins order of [`PakSet::open`] is itself not decoded.
pub fn pak_files_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let rd = std::fs::read_dir(dir).map_err(|e| PakError::Io(dir.to_path_buf(), e))?;
    let mut found: Vec<(bool, u64, String)> = Vec::new();
    for ent in rd {
        let ent = ent.map_err(|e| PakError::Io(dir.to_path_buf(), e))?;
        let name = ent.file_name().to_string_lossy().into_owned();
        if let Some((chunk, patch)) = parse_pak_name(&name) {
            found.push((patch, chunk, name));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| cmp_utf16(&a.2, &b.2)));
    Ok(found.into_iter().map(|(_, _, n)| dir.join(n)).collect())
}

/// `pakchunk<n>-WindowsNoEditor[_<m>_P | _P].pak` (any case): the chunk number and whether it is a patch pak.
fn parse_pak_name(name: &str) -> Option<(u64, bool)> {
    let low = name.to_ascii_lowercase();
    let rest = low.strip_prefix("pakchunk")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let chunk: u64 = rest[..digits].parse().ok()?;
    let rest = rest[digits..].strip_prefix("-windowsnoeditor")?.strip_suffix(".pak")?;
    let patch = match rest {
        "" => false,
        "_p" => true,
        r => {
            let m = r.strip_prefix('_')?.strip_suffix("_p")?;
            if m.is_empty() || !m.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            true
        }
    };
    Some((chunk, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pak_names() {
        assert_eq!(parse_pak_name("pakchunk12-WindowsNoEditor.pak"), Some((12, false)));
        assert_eq!(parse_pak_name("PAKCHUNK0-windowsnoeditor_P.PAK"), Some((0, true)));
        assert_eq!(parse_pak_name("pakchunk3-WindowsNoEditor_2_P.pak"), Some((3, true)));
        assert_eq!(parse_pak_name("pakchunk3-WindowsNoEditor_P_P.pak"), None);
        assert_eq!(parse_pak_name("pakchunk-WindowsNoEditor.pak"), None);
        assert_eq!(parse_pak_name("pakchunk1-WindowsNoEditor.sig"), None);
    }

    #[test]
    fn utf16_order() {
        assert_eq!(cmp_utf16("a/B", "a/a"), Ordering::Less);
        assert_eq!(cmp_utf16("\u{10000}", "\u{FFFF}"), Ordering::Less);
        assert_eq!("\u{10000}".cmp("\u{FFFF}"), Ordering::Greater);
        assert!(same_lower("Engine/Content/A.uasset", "engine/content/a.UASSET"));
        assert_eq!(lower_hash("ÄB"), lower_hash("äb"));
    }
}
