//! Media to samples: a `sound` entry's Vorbis packets through lewton. A short medium is decoded whole on a worker
//! when its scope loads; a long one (a second or more: `cache::STREAM_FROM_SECS`) and a streamed one (music) are
//! kept compressed and decoded packet by packet on the streamer thread as they play (lewton allocates per packet, so
//! not on the mixer thread).
//!
//! A packet's decoded output starts at the frame the previous packet's granule names: the first packet of a decode
//! gives none (it only primes the window overlap), so a seek decodes from the packet before the one that holds the
//! frame and lands on it exactly.

use std::sync::Arc;

use lewton::audio::{PreviousWindowRight, get_decoded_sample_count, read_audio_packet_generic};
use lewton::header::{IdentHeader, SetupHeader, read_header_ident, read_header_setup};
use mvs_data::sound::{Id, Medium, MediumHeader, decode_medium};

/// A medium decoded: planar f32, channel `c` at `data[c * frames..(c + 1) * frames]`.
#[derive(Clone, Debug, Default)]
pub struct Pcm {
    pub id: Id,
    pub rate: u32,
    pub channels: u16,
    pub frames: u32,
    pub loop_points: Option<[u32; 2]>,
    pub data: Box<[f32]>,
}

impl Pcm {
    pub fn channel(&self, c: usize) -> &[f32] {
        let n = self.frames as usize;
        &self.data[c * n..(c + 1) * n]
    }
}

/// A medium kept compressed and decoded as it plays (the streamer's): its entry and the setup header parsed for it.
pub struct Coded {
    pub id: Id,
    pub rate: u32,
    pub channels: u16,
    pub frames: u32,
    pub entry: Box<[u8]>,
    setup: Arc<SetupHeader>,
}

impl std::fmt::Debug for Coded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coded").field("id", &self.id).field("bytes", &self.entry.len()).finish()
    }
}

impl Coded {
    /// Medium `id`'s entry, its setup header parsed from `setup`'s bytes.
    pub fn new(id: Id, entry: Box<[u8]>, setup: &[u8]) -> Result<Coded, DecodeError> {
        let m = decode_medium(&entry, &format!("medium {id}"))?;
        let ident = read_ident(id, &m.header)?;
        let setup = Arc::new(read_setup(id, &ident, setup)?);
        Coded::with_setup(id, entry, setup)
    }

    /// Medium `id`'s entry with its setup header parsed already (shared by the media of one setup and ident).
    fn with_setup(id: Id, entry: Box<[u8]>, setup: Arc<SetupHeader>) -> Result<Coded, DecodeError> {
        let h = decode_medium(&entry, &format!("medium {id}"))?.header;
        Ok(Coded { id, rate: h.rate, channels: h.channels, frames: h.frames, entry, setup })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error(transparent)]
    Sound(#[from] mvs_data::sound::SoundError),
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
    #[error("{0}: vorbis: {1}")]
    Vorbis(String, String),
}

/// The decoder's headers, read from a medium's ident header and its shared setup header (parsed once per setup
/// header by the cache: cache.rs).
struct Headers {
    ident: IdentHeader,
    setup: Arc<SetupHeader>,
}

/// A medium's ident header, its channels checked against its entry's.
fn read_ident(id: Id, h: &MediumHeader) -> Result<IdentHeader, DecodeError> {
    let bad = |e: String| DecodeError::Vorbis(format!("medium {id}"), e);
    let ident = read_header_ident(&h.ident).map_err(|e| bad(format!("ident header: {e:?}")))?;
    if u32::from(ident.audio_channels) != u32::from(h.channels) {
        return Err(bad(format!("ident has {} channels, the entry {}", ident.audio_channels, h.channels)));
    }
    Ok(ident)
}

/// A setup header's bytes parsed for a medium of `ident`'s channels and block sizes.
fn read_setup(id: Id, ident: &IdentHeader, setup: &[u8]) -> Result<SetupHeader, DecodeError> {
    read_header_setup(setup, ident.audio_channels, (ident.blocksize_0, ident.blocksize_1))
        .map_err(|e| DecodeError::Vorbis(format!("medium {id}"), format!("setup header: {e:?}")))
}

impl Headers {
    fn read(id: Id, h: &MediumHeader, setup: &[u8]) -> Result<Headers, DecodeError> {
        let ident = read_ident(id, h)?;
        let setup = Arc::new(read_setup(id, &ident, setup)?);
        Ok(Headers { ident, setup })
    }

    fn packet(&self, id: Id, i: usize, p: &[u8], pwr: &mut PreviousWindowRight) -> Result<Vec<Vec<f32>>, DecodeError> {
        read_audio_packet_generic::<Vec<Vec<f32>>>(&self.ident, &self.setup, p, pwr)
            .map_err(|e| DecodeError::Vorbis(format!("medium {id}"), format!("packet {i}: {e:?}")))
    }
}

/// Copies a packet's output that starts at medium frame `at` into the planar `out` of `frames` frames per channel.
fn place(out: &mut [f32], frames: usize, at: usize, pcm: &[Vec<f32>]) {
    for (c, ch) in pcm.iter().enumerate() {
        let dst = &mut out[c * frames..(c + 1) * frames];
        if at >= frames {
            return;
        }
        let n = ch.len().min(frames - at);
        dst[at..at + n].copy_from_slice(&ch[..n]);
    }
}

/// Decodes a whole medium (`setup`: its shared setup header's bytes), trimmed to its frames.
pub fn decode(id: Id, medium: &Medium, setup: &[u8]) -> Result<Pcm, DecodeError> {
    decode_with(id, medium, &Headers::read(id, &medium.header, setup)?)
}

/// [`decode`] with the headers read.
fn decode_with(id: Id, medium: &Medium, hd: &Headers) -> Result<Pcm, DecodeError> {
    let h = &medium.header;
    let frames = h.frames as usize;
    let channels = usize::from(h.channels);
    let mut data = vec![0.0f32; frames * channels].into_boxed_slice();
    let mut pwr = PreviousWindowRight::new();
    let mut at = 0usize;
    for i in 0..medium.packets() {
        let pcm = hd.packet(id, i, medium.packet(i), &mut pwr)?;
        let n = pcm.first().map_or(0, Vec::len);
        if i == 0 {
            // (the first packet primes the overlap: no output)
            at = medium.granule(0) as usize;
            continue;
        }
        place(&mut data, frames, at, &pcm);
        at += n;
    }
    Ok(Pcm { id, rate: h.rate, channels: h.channels, frames: h.frames, loop_points: h.loop_points, data })
}

/// Reads a medium's entry and its setup from the `sound` pack and decodes it.
pub fn load(pack: &mvs_store::Pack, id: Id, path: &str) -> Result<Pcm, DecodeError> {
    let bytes = pack.bytes(path)?;
    let m = decode_medium(&bytes, path)?;
    let setup = pack.bytes(&m.header.setup)?;
    decode(id, &m, &setup)
}

/// A streamed medium: its entry and the decoder's place, read ahead of the voice.
pub struct Stream {
    pub(crate) id: Id,
    coded: Arc<Coded>,
    hd: Headers,
    pub(crate) channels: usize,
    pub(crate) frames: u32,
    /// Each packet's byte range in `entry`.
    packets: Box<[[u32; 2]]>,
    /// The medium frame each packet's decoded output ends at. The decoder gives a long block's finished samples
    /// before a short one early, so this runs ahead of the entry's granules there.
    ends: Box<[u32]>,
    pwr: PreviousWindowRight,
    /// The next packet to decode.
    next: usize,
    /// The last decoded packet's output, planar, and the medium frame of its first sample.
    buf: Vec<Vec<f32>>,
    buf_at: u32,
    /// The medium frame the next read gives.
    pub(crate) pos: u32,
}

impl Stream {
    /// The stream of a medium, at frame `from`.
    pub fn open(coded: Arc<Coded>, from: u32) -> Result<Stream, DecodeError> {
        let id = coded.id;
        let what = format!("medium {id}");
        let m = decode_medium(&coded.entry, &what)?;
        let hd = Headers { ident: read_ident(id, &m.header)?, setup: coded.setup.clone() };
        let base = coded.entry.as_ptr() as usize;
        let packets = (0..m.packets())
            .map(|i| {
                let p = m.packet(i);
                let s = u32::try_from(p.as_ptr() as usize - base).expect("an entry under 4 GB");
                [s, s + u32::try_from(p.len()).expect("a packet under 4 GB")]
            })
            .collect();
        let mut end = 0u32;
        let ends = (0..m.packets())
            .map(|i| {
                if i > 0 {
                    let n = get_decoded_sample_count(&hd.ident, &hd.setup, m.packet(i)).unwrap_or(0);
                    end = end.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
                }
                end
            })
            .collect();
        let h = m.header;
        let mut s = Stream {
            id,
            coded: coded.clone(),
            hd,
            channels: usize::from(h.channels),
            frames: h.frames,
            packets,
            ends,
            pwr: PreviousWindowRight::new(),
            next: 0,
            buf: Vec::new(),
            buf_at: 0,
            pos: 0,
        };
        s.seek(from);
        Ok(s)
    }

    /// Decodes the next packet into `buf`; false at the stream's end or on a bad packet (the stream ends there).
    fn decode_next(&mut self) -> bool {
        let Some(&[a, b]) = self.packets.get(self.next) else { return false };
        let i = self.next;
        let first = self.pwr.is_empty();
        let pkt = &self.coded.entry[a as usize..b as usize];
        match self.hd.packet(self.id, i, pkt, &mut self.pwr) {
            Ok(pcm) => {
                self.next += 1;
                if first {
                    // a primed decode starts its output where this packet's ends
                    self.buf.clear();
                    self.buf_at = self.ends[i];
                } else {
                    self.buf_at = self.buf_at.saturating_add(self.buf.first().map_or(0, |c| c.len() as u32));
                    self.buf = pcm;
                }
                true
            }
            Err(e) => {
                eprintln!("audio: {e}");
                self.next = self.packets.len();
                false
            }
        }
    }

    /// Fills `out` (planar, `out.len()` channels of equal length) from the stream; the frames written (fewer at its
    /// end).
    pub fn read(&mut self, out: &mut [&mut [f32]]) -> usize {
        let want = out.first().map_or(0, |c| c.len());
        let mut done = 0;
        while done < want && self.pos < self.frames {
            let have_end = self.buf_at + self.buf.first().map_or(0, |c| c.len() as u32);
            if self.pos >= have_end || self.pos < self.buf_at {
                if !self.decode_next() {
                    break;
                }
                continue;
            }
            let from = (self.pos - self.buf_at) as usize;
            let n = ((have_end - self.pos) as usize).min(want - done).min((self.frames - self.pos) as usize);
            for (c, o) in out.iter_mut().enumerate() {
                match self.buf.get(c) {
                    Some(src) => o[done..done + n].copy_from_slice(&src[from..from + n]),
                    None => o[done..done + n].fill(0.0),
                }
            }
            done += n;
            self.pos += n as u32;
        }
        done
    }

    /// Moves to frame `to` (a loop's start), exact through the packets' output ends.
    pub fn seek(&mut self, to: u32) {
        // the packet whose output holds `to`: the first whose output ends past it; the decode primes on the one
        // before it
        let i = self.ends.partition_point(|&e| e <= to).max(1);
        self.next = i - 1;
        self.pwr = PreviousWindowRight::new();
        self.buf.clear();
        self.buf_at = 0;
        self.pos = to.min(self.frames);
        // prime: its output is none, its end is where the next packet's starts
        self.decode_next();
    }
}

pub mod cache;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod worker;
