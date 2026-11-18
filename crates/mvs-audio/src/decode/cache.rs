//! The decoded media and the Vorbis setup headers, kept across scopes and matches: a medium is decoded once and
//! shared by every scope that holds it (a Lab restart with the same fighters decodes nothing), and each of the
//! sound kind's few setup headers is parsed once (its codebooks are most of a short medium's decode).
//!
//! The decoded media a scope holds stay while it holds them; past that the cache keeps the most recently used up to
//! its budget (bytes of decoded samples, scopes' media counted), and lets the rest go at [`MediaCache::trim`].

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use lewton::header::SetupHeader;
use mvs_data::sound::{Id, decode_medium};
use mvs_store::{Cache, CacheStats};

use super::{DecodeError, Headers, Pcm, decode_with, read_ident, read_setup};

/// The decoded media and the setup headers.
// A scope's media are decoded once and kept with their setup headers (one parsed per medium). Kept as f32: 16-bit
// storage would halve the bytes and is not taken (the mixer reads f32).
pub struct MediaCache {
    setups: Cache,
    pcm: Cache,
    budget: AtomicUsize,
}

impl std::fmt::Debug for MediaCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaCache").field("pcm", &self.pcm).field("budget", &self.budget).finish()
    }
}

/// A medium from the cache: its samples, and whether this call decoded them.
#[derive(Clone, Debug)]
pub struct Got {
    pub pcm: Arc<Pcm>,
    pub decoded: bool,
}

impl MediaCache {
    /// A cache keeping at most `budget` bytes of decoded samples past what the scopes hold.
    pub fn new(budget: usize) -> MediaCache {
        MediaCache { setups: Cache::new(), pcm: Cache::new(), budget: AtomicUsize::new(budget) }
    }

    /// The budget for this PC: a sixteenth of its memory, between 192 and 768 MB (a match holds 270 to 470 MB of
    /// decoded media: a PC of 8 GB or more keeps the last line-up's whole for the next restart).
    pub fn default_budget() -> usize {
        (physical_memory().unwrap_or(8 << 30) / 16).clamp(192 << 20, 768 << 20)
    }

    pub fn budget(&self) -> usize {
        self.budget.load(Ordering::Relaxed)
    }

    pub fn set_budget(&self, bytes: usize) {
        self.budget.store(bytes, Ordering::Relaxed);
    }

    /// Medium `id` (its entry `path` of the `sound` pack), decoded on the first call; any thread.
    pub fn medium(&self, pack: &mvs_store::Pack, id: Id, path: &str) -> Result<Got, DecodeError> {
        let mut decoded = false;
        let pcm = self.pcm.get_or_weighed(
            &id.to_string(),
            || {
                decoded = true;
                self.decode(pack, id, path)
            },
            |p: &Pcm| p.data.len() * size_of::<f32>(),
        )?;
        Ok(Got { pcm, decoded })
    }

    fn decode(&self, pack: &mvs_store::Pack, id: Id, path: &str) -> Result<Pcm, DecodeError> {
        let bytes = pack.bytes(path)?;
        let m = decode_medium(&bytes, path)?;
        let ident = read_ident(id, &m.header)?;
        let key = format!("{}|{}|{}|{}", m.header.setup, ident.audio_channels, ident.blocksize_0, ident.blocksize_1);
        let setup: Arc<SetupHeader> =
            self.setups.get_or(&key, || read_setup(id, &ident, &pack.bytes(&m.header.setup)?))?;
        decode_with(id, &m, &Headers { ident, setup })
    }

    /// Lets the decoded media no scope holds go, least recently used first, down to the budget; the bytes let go.
    pub fn trim(&self) -> usize {
        self.pcm.trim(self.budget())
    }

    /// The decoded media held: entries, bytes, and the bytes no scope holds.
    pub fn stats(&self) -> CacheStats {
        self.pcm.stats()
    }
}

#[cfg(windows)]
fn physical_memory() -> Option<usize> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: a status struct with its length set, as the call asks.
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    usize::try_from(m.ullTotalPhys).ok()
}

#[cfg(not(windows))]
fn physical_memory() -> Option<usize> {
    None
}
