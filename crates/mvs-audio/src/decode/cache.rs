//! The media kept across scopes and matches: the short ones decoded, the long ones compressed (decoded as they play
//! by the streamer), and the Vorbis setup headers. A medium is read once and shared by every scope that holds it (a
//! Lab restart with the same fighters reads and decodes nothing), and each of the sound kind's few setup headers is
//! parsed once (its codebooks are most of a short medium's decode).
//!
//! The media a scope holds stay while it holds them; [`MediaCache::trim`] lets the rest go (at a match's install,
//! once its scopes hold theirs).

use std::sync::Arc;

use lewton::header::SetupHeader;
use mvs_data::sound::{Id, Medium, decode_medium};
use mvs_store::{Cache, CacheStats};

use super::{Coded, DecodeError, Headers, Pcm, decode_with, read_ident, read_setup};

/// The shortest medium kept compressed and decoded as it plays, seconds; a shorter one is decoded when its scope
/// loads and kept decoded.
// Deviation: the build decodes every Vorbis voice as it plays (a Data source holds its medium compressed in the
// bank's memory). The port's mixer thread may not allocate and lewton does, so a streamed voice is decoded ahead on
// the streamer thread and starts silent until its first frames are there (about 6 ms on average in a
// replay, a device period); the short media, most of the hits and steps and played most often, are decoded up front so they
// start on their sample. A match's sound is about 0.2 GB decoded and 0.05 GB compressed this way, against about
// 1 GB with every medium decoded.
pub const STREAM_FROM_SECS: u32 = 1;

/// The media and the setup headers.
#[derive(Default)]
pub struct MediaCache {
    setups: Cache,
    pcm: Cache,
    coded: Cache,
}

impl std::fmt::Debug for MediaCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaCache").field("pcm", &self.pcm).field("coded", &self.coded).finish()
    }
}

/// A medium from the cache: its samples, and whether this call decoded them.
#[derive(Clone, Debug)]
pub struct Got {
    pub pcm: Arc<Pcm>,
    pub decoded: bool,
}

/// How a scope holds a medium.
#[derive(Clone, Debug)]
pub enum Hold {
    Pcm(Got),
    Coded(Arc<Coded>),
}

/// Whether a medium of this header plays streamed.
fn long(h: &mvs_data::sound::MediumHeader) -> bool {
    u64::from(h.frames) >= u64::from(h.rate) * u64::from(STREAM_FROM_SECS)
}

impl MediaCache {
    pub fn new() -> MediaCache {
        MediaCache::default()
    }

    /// Medium `id` (its entry `path` of the `sound` pack), decoded on the first call; any thread.
    pub fn medium(&self, pack: &mvs_store::Pack, id: Id, path: &str) -> Result<Got, DecodeError> {
        let mut decoded = false;
        let pcm = self.pcm.get_or_weighed(
            &id.to_string(),
            || {
                decoded = true;
                let bytes = pack.bytes(path)?;
                self.decode(pack, id, &decode_medium(&bytes, path)?)
            },
            |p: &Pcm| p.data.len() * size_of::<f32>(),
        )?;
        Ok(Got { pcm, decoded })
    }

    /// Medium `id` compressed, read on the first call; any thread.
    pub fn coded(&self, pack: &mvs_store::Pack, id: Id, path: &str) -> Result<Arc<Coded>, DecodeError> {
        self.coded.get_or_weighed(
            &id.to_string(),
            || self.read_coded(pack, id, pack.bytes(path)?),
            |c: &Coded| c.entry.len(),
        )
    }

    /// Medium `id` as a scope's load holds it: compressed when it is long or `streamed`, else decoded.
    pub fn hold(&self, pack: &mvs_store::Pack, id: Id, path: &str, streamed: bool) -> Result<Hold, DecodeError> {
        let key = id.to_string();
        if let Some(c) = self.coded.peek::<Coded>(&key) {
            return Ok(Hold::Coded(c));
        }
        if !streamed && let Some(pcm) = self.pcm.peek::<Pcm>(&key) {
            return Ok(Hold::Pcm(Got { pcm, decoded: false }));
        }
        let bytes = pack.bytes(path)?;
        let m = decode_medium(&bytes, path)?;
        if streamed || long(&m.header) {
            drop(m);
            let c = self.coded.get_or_weighed(&key, || self.read_coded(pack, id, bytes), |c: &Coded| c.entry.len())?;
            return Ok(Hold::Coded(c));
        }
        let mut decoded = false;
        let pcm = self.pcm.get_or_weighed(
            &key,
            || {
                decoded = true;
                self.decode(pack, id, &m)
            },
            |p: &Pcm| p.data.len() * size_of::<f32>(),
        )?;
        Ok(Hold::Pcm(Got { pcm, decoded }))
    }

    /// Medium `id` compressed when it is long (a lazy scope's load: a long medium is held at once, a short one is
    /// decoded when first posted).
    pub fn long(&self, pack: &mvs_store::Pack, id: Id, path: &str) -> Result<Option<Arc<Coded>>, DecodeError> {
        let key = id.to_string();
        if let Some(c) = self.coded.peek::<Coded>(&key) {
            return Ok(Some(c));
        }
        if self.pcm.peek::<Pcm>(&key).is_some() {
            return Ok(None);
        }
        let bytes = pack.bytes(path)?;
        if !long(&decode_medium(&bytes, path)?.header) {
            return Ok(None);
        }
        Ok(Some(self.coded.get_or_weighed(&key, || self.read_coded(pack, id, bytes), |c: &Coded| c.entry.len())?))
    }

    fn setup(&self, pack: &mvs_store::Pack, id: Id, m: &Medium) -> Result<Headers, DecodeError> {
        let ident = read_ident(id, &m.header)?;
        let key = format!("{}|{}|{}|{}", m.header.setup, ident.audio_channels, ident.blocksize_0, ident.blocksize_1);
        let setup: Arc<SetupHeader> =
            self.setups.get_or(&key, || read_setup(id, &ident, &pack.bytes(&m.header.setup)?))?;
        Ok(Headers { ident, setup })
    }

    fn decode(&self, pack: &mvs_store::Pack, id: Id, m: &Medium) -> Result<Pcm, DecodeError> {
        decode_with(id, m, &self.setup(pack, id, m)?)
    }

    fn read_coded(&self, pack: &mvs_store::Pack, id: Id, bytes: Vec<u8>) -> Result<Coded, DecodeError> {
        let setup = self.setup(pack, id, &decode_medium(&bytes, &format!("medium {id}"))?)?.setup;
        Coded::with_setup(id, bytes.into_boxed_slice(), setup)
    }

    /// Lets the media no scope holds go; the bytes let go.
    pub fn trim(&self) -> usize {
        self.pcm.trim(0) + self.coded.trim(0)
    }

    /// The media held, decoded and compressed: entries, bytes, and the bytes no scope holds.
    pub fn stats(&self) -> CacheStats {
        let (a, b) = (self.pcm.stats(), self.coded.stats());
        CacheStats {
            entries: a.entries + b.entries,
            bytes: a.bytes + b.bytes,
            unheld_bytes: a.unheld_bytes + b.unheld_bytes,
        }
    }
}
