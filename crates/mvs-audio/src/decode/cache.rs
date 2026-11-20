//! The decoded media and the Vorbis setup headers, kept across scopes and matches: a medium is decoded once and
//! shared by every scope that holds it (a Lab restart with the same fighters decodes nothing), and each of the
//! sound kind's few setup headers is parsed once (its codebooks are most of a short medium's decode).
//!
//! The decoded media a scope holds stay while it holds them; [`MediaCache::trim`] lets the rest go (at a match's
//! install, once its scopes hold theirs).

use std::sync::Arc;

use lewton::header::SetupHeader;
use mvs_data::sound::{Id, decode_medium};
use mvs_store::{Cache, CacheStats};

use super::{DecodeError, Headers, Pcm, decode_with, read_ident, read_setup};

/// The decoded media and the setup headers.
// A scope's media are decoded once and kept with their setup headers (one parsed per medium). Kept as f32: 16-bit
// storage would halve the bytes and is not taken (the mixer reads f32).
#[derive(Default)]
pub struct MediaCache {
    setups: Cache,
    pcm: Cache,
}

impl std::fmt::Debug for MediaCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaCache").field("pcm", &self.pcm).finish()
    }
}

/// A medium from the cache: its samples, and whether this call decoded them.
#[derive(Clone, Debug)]
pub struct Got {
    pub pcm: Arc<Pcm>,
    pub decoded: bool,
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

    /// Lets the decoded media no scope holds go; the bytes let go.
    pub fn trim(&self) -> usize {
        self.pcm.trim(0)
    }

    /// The decoded media held: entries, bytes, and the bytes no scope holds.
    pub fn stats(&self) -> CacheStats {
        self.pcm.stats()
    }
}
