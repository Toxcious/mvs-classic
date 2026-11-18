//! A Wwise medium's file (a wem: RIFF WAVE with Wwise's chunks). Every medium of the build is Wwise Vorbis at bank
//! version 135: a 0x42-byte 'fmt ' holding WAVEFORMATEXTENSIBLE's first 24 bytes and the Vorbis header (ww2ogg's
//! embedded `vorb`, size -1).
//!
//! Read: 'fmt ' (format tag 0xFFFF, channels, rate, the channel configuration, the Vorbis header), 'smpl' (the loop
//! points), 'akd ' (AkFileParser::AnalysisData: loudness normalisation, envelope) and 'data'. 'cue ' and 'LIST' are
//! skipped.

/// The 'fmt ' chunk's Vorbis header (`vorb`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WemVorbis {
    /// dwTotalPCMFrames: the medium's length in samples per channel.
    pub samples: u32,
    /// The loop info's packet offsets and extras (vorb +0x04 .. +0x0F). ww2ogg tells mod_packets from the first.
    pub loop_start_packet: u32,
    pub loop_end_packet: u32,
    pub loop_begin_extra: u16,
    pub loop_end_extra: u16,
    /// dwSeekTableSize (+0x10): the setup packet's offset in 'data', after the seek table.
    pub seek_table_size: u32,
    /// dwVorbisDataOffset (+0x14): the first audio packet's offset in 'data'.
    pub data_offset: u32,
    /// uMaxPacketSize (+0x18).
    pub max_packet: u16,
    /// uLastGranuleExtra (+0x1A): samples the last packet decodes past `samples`.
    pub last_granule_extra: u16,
    /// dwDecodeAllocSize (+0x1C), dwDecodeX64AllocSize (+0x20).
    pub decode_alloc: u32,
    pub decode_alloc_x64: u32,
    /// The setup packet's hash (+0x24; ww2ogg's uid).
    pub uid: u32,
    /// Block sizes as powers of two (+0x28, +0x29).
    pub blocksize0_pow: u8,
    pub blocksize1_pow: u8,
}

/// The 'smpl' loop's dwStart and dwEnd as stored (samples): the end is inclusive (RIFF's rule), so the exclusive loop
/// end is `end + 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WemLoop {
    pub start: u32,
    pub end: u32,
}

/// The 'akd ' chunk (AkFileParser::AnalysisData).
#[derive(Clone, Debug, PartialEq)]
pub struct WemAnalysis {
    /// Loudness normalisation gain (linear; > 0 when set).
    pub norm_gain: f32,
    /// Downmix normalisation gain (linear).
    pub downmix_gain: f32,
    /// The envelope's highest level (dB), when it has points.
    pub peak: Option<f32>,
    /// [sample, attenuation dB] points of the loudness envelope.
    pub env: Vec<[u32; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wem {
    pub channels: u16,
    pub rate: u32,
    /// uChannelConfig: channels | type << 8 | speaker mask << 12.
    pub channel_config: u32,
    /// The speaker mask of a source with more than two channels (type 1, one bit per channel).
    pub mask: Option<u32>,
    pub vorbis: WemVorbis,
    pub loops: Vec<WemLoop>,
    pub analysis: Option<WemAnalysis>,
    /// Where 'data' lies in the file.
    pub data_offset: usize,
    pub data_size: usize,
}

pub(super) fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub(super) fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}

/// Parses a wem. Fails, naming `what`, for a file that is not Wwise Vorbis as this build stores it.
pub fn parse_wem(bytes: &[u8], what: &str) -> Result<Wem, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{what}: not a RIFF WAVE file"));
    }
    let mut fmt: Option<(u16, u32, u32, Option<u32>, WemVorbis)> = None;
    let mut loops = Vec::new();
    let mut analysis = None;
    let mut data = None;
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(bytes, pos + 4) as usize;
        let at = pos + 8;
        let name = String::from_utf8_lossy(id).into_owned();
        if at + size > bytes.len() {
            return Err(format!("{what}: chunk '{name}' runs past the file"));
        }
        match id {
            b"fmt " => {
                if size != 0x42 {
                    return Err(format!("{what}: 'fmt ' of {size} bytes, not Wwise Vorbis (0x42)"));
                }
                let tag = u16_at(bytes, at);
                if tag != 0xffff {
                    return Err(format!("{what}: format tag {tag:x}, not Wwise Vorbis (ffff)"));
                }
                let channels = u16_at(bytes, at + 2);
                let config = u32_at(bytes, at + 20);
                let mut mask = None;
                if channels > 2 {
                    let m = config >> 12;
                    if config & 0xff != u32::from(channels)
                        || (config >> 8) & 0xf != 1
                        || m.count_ones() != u32::from(channels)
                    {
                        return Err(format!("{what}: channel config {config:x} is not a standard speaker mask"));
                    }
                    mask = Some(m);
                }
                let b = at + 24;
                let vorbis = WemVorbis {
                    samples: u32_at(bytes, b),
                    loop_start_packet: u32_at(bytes, b + 0x04),
                    loop_end_packet: u32_at(bytes, b + 0x08),
                    loop_begin_extra: u16_at(bytes, b + 0x0c),
                    loop_end_extra: u16_at(bytes, b + 0x0e),
                    seek_table_size: u32_at(bytes, b + 0x10),
                    data_offset: u32_at(bytes, b + 0x14),
                    max_packet: u16_at(bytes, b + 0x18),
                    last_granule_extra: u16_at(bytes, b + 0x1a),
                    decode_alloc: u32_at(bytes, b + 0x1c),
                    decode_alloc_x64: u32_at(bytes, b + 0x20),
                    uid: u32_at(bytes, b + 0x24),
                    blocksize0_pow: bytes[b + 0x28],
                    blocksize1_pow: bytes[b + 0x29],
                };
                fmt = Some((channels, u32_at(bytes, at + 4), config, mask, vorbis));
            }
            b"smpl" => {
                if size < 36 {
                    return Err(format!("{what}: 'smpl' of {size} bytes"));
                }
                let n = u32_at(bytes, at + 28) as usize;
                if 36 + 24 * n > size {
                    return Err(format!("{what}: 'smpl' names {n} loops in {size} bytes"));
                }
                for i in 0..n {
                    loops.push(WemLoop {
                        start: u32_at(bytes, at + 36 + 24 * i + 8),
                        end: u32_at(bytes, at + 36 + 24 * i + 12),
                    });
                }
            }
            b"akd " => {
                if size < 12 {
                    return Err(format!("{what}: 'akd ' of {size} bytes"));
                }
                let n = u32_at(bytes, at + 8) as usize;
                if n != 0 && 16 + 6 * n > size {
                    return Err(format!("{what}: 'akd ' names {n} envelope points in {size} bytes"));
                }
                let env = (0..n)
                    .map(|i| [u32_at(bytes, at + 16 + 6 * i), u32::from(u16_at(bytes, at + 20 + 6 * i))])
                    .collect();
                analysis = Some(WemAnalysis {
                    norm_gain: f32_at(bytes, at),
                    downmix_gain: f32_at(bytes, at + 4),
                    peak: (n != 0).then(|| f32_at(bytes, at + 12)),
                    env,
                });
            }
            b"data" => data = Some((at, size)),
            _ => {}
        }
        pos = at + size + (size & 1);
    }
    let Some((channels, rate, channel_config, mask, vorbis)) = fmt else {
        return Err(format!("{what}: no 'fmt ' chunk"));
    };
    let Some((data_offset, data_size)) = data else {
        return Err(format!("{what}: no 'data' chunk"));
    };
    Ok(Wem { channels, rate, channel_config, mask, vorbis, loops, analysis, data_offset, data_size })
}
