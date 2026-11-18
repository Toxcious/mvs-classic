//! A port of ww2ogg 0.24 (Adam Gashlin / hcs; BSD licence): a Wwise Vorbis wem's packets rebuilt as standard Vorbis
//! packets, with no re-encode. It leaves out the comment header and the Ogg pages.
//!
//! It covers the one layout every medium of this build has: RIFF (not RIFX), a 0x42-byte 'fmt ' holding the `vorb`
//! header at +0x18 (two-byte packet headers with no granule, mod_packets by the test on +0x04, the setup hash and block
//! sizes at +0x24), a stripped setup packet and the external codebook library (`packed_codebooks_aoTuV_603.bin`). A wem
//! that needs another path fails by name. What it gives:
//! - identification: channels, rate, the nominal bit rate from 'fmt ' (bytes per second times 8), the block sizes;
//! - setup: the codebooks expanded from the library by their 10-bit ids, the time placeholder, then the floors
//!   (type 1), residues, mappings (type 0) and modes (window and transform 0) widened to their standard fields;
//! - audio: each packet with the packet type bit, the mode number, and for a long window the previous and next window
//!   flags put in front of its bits.

/// The packed codebook library: the books back to back, then one u32 offset per book and one for the end
/// (codebook.cpp codebook_library).
pub struct CodebookLibrary {
    data: Vec<u8>,
    offsets: Vec<u32>,
}

impl CodebookLibrary {
    /// A library from ww2ogg's .bin layout: the last u32 is the offset table's place, the table runs to the end.
    pub fn from_bin(bin: &[u8]) -> Result<CodebookLibrary, String> {
        let bad = || "ww2ogg: not a packed codebook file".to_owned();
        if bin.len() < 4 {
            return Err(bad());
        }
        let at = super::wem::u32_at(bin, bin.len() - 4) as usize;
        if at > bin.len() || !(bin.len() - at).is_multiple_of(4) || (bin.len() - at) / 4 < 2 {
            return Err(bad());
        }
        let offsets: Vec<u32> = (0..(bin.len() - at) / 4).map(|i| super::wem::u32_at(bin, at + 4 * i)).collect();
        Ok(CodebookLibrary { data: bin[..at].to_vec(), offsets })
    }

    /// The bytes of book `i`, `None` for an id past the last book (get_codebook / get_codebook_size).
    fn book(&self, i: usize) -> Option<&[u8]> {
        if i + 1 >= self.offsets.len() {
            return None;
        }
        self.data.get(self.offsets[i] as usize..self.offsets[i + 1] as usize)
    }
}

/// Bit_stream: bits LSB first from `b` at `pos`. Reading past the end fails (Out_of_bits).
struct BitReader<'a> {
    b: &'a [u8],
    pos: usize,
    cur: u8,
    left: u32,
    total: usize,
}

impl<'a> BitReader<'a> {
    fn new(b: &'a [u8], pos: usize) -> BitReader<'a> {
        BitReader { b, pos, cur: 0, left: 0, total: 0 }
    }

    fn bit(&mut self) -> Result<u32, String> {
        if self.left == 0 {
            let Some(&c) = self.b.get(self.pos) else { return Err("ww2ogg: out of bits".into()) };
            self.cur = c;
            self.pos += 1;
            self.left = 8;
        }
        self.total += 1;
        self.left -= 1;
        Ok(u32::from((self.cur >> (7 - self.left)) & 1))
    }

    fn bits(&mut self, n: u32) -> Result<u32, String> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }
}

/// Bit_oggstream's bit packing (LSB first) into one packet; `end` pads the last byte with zeros.
struct BitWriter {
    buf: Vec<u8>,
    acc: u32,
    stored: u32,
}

impl BitWriter {
    fn new(size: usize) -> BitWriter {
        BitWriter { buf: Vec::with_capacity(size.max(16)), acc: 0, stored: 0 }
    }

    fn bits(&mut self, v: u32, n: u32) {
        for i in 0..n {
            if (v >> i) & 1 != 0 {
                self.acc |= 1 << self.stored;
            }
            self.stored += 1;
            if self.stored == 8 {
                self.buf.push(self.acc as u8);
                self.acc = 0;
                self.stored = 0;
            }
        }
    }

    /// `src` at the current bit position.
    fn bytes(&mut self, src: &[u8]) {
        let s = self.stored;
        if s == 0 {
            self.buf.extend_from_slice(src);
            return;
        }
        let mut acc = self.acc;
        for &c in src {
            let c = u32::from(c);
            self.buf.push(((acc | (c << s)) & 0xff) as u8);
            acc = c >> (8 - s);
        }
        self.acc = acc;
    }

    fn end(mut self) -> Result<Vec<u8>, String> {
        if self.stored != 0 {
            self.buf.push(self.acc as u8);
        }
        if self.buf.len() >= 255 * 255 {
            return Err("ww2ogg: ran out of space in an Ogg packet".into());
        }
        Ok(self.buf)
    }
}

/// ilog (Tremor): the bits of v.
fn ilog(mut v: u32) -> u32 {
    let mut r = 0;
    while v != 0 {
        r += 1;
        v >>= 1;
    }
    r
}

/// _book_maptype1_quantvals (Tremor), the products in doubles.
fn quantvals(entries: u32, dims: u32) -> Result<u32, String> {
    if dims == 0 {
        return Err("ww2ogg: a lookup table of a book with no dimensions".into());
    }
    let bits = ilog(entries);
    let shift = (i64::from(bits) - 1) * (i64::from(dims) - 1) / i64::from(dims);
    let mut vals = i64::from(entries) >> shift.clamp(0, 63);
    loop {
        let mut acc = 1f64;
        let mut acc1 = 1f64;
        for _ in 0..dims {
            acc *= vals as f64;
            acc1 *= (vals + 1) as f64;
        }
        let e = f64::from(entries);
        if acc <= e && acc1 > e {
            return Ok(vals as u32);
        }
        if acc > e {
            vals -= 1;
        } else {
            vals += 1;
        }
    }
}

/// codebook_library::rebuild: a packed book (4-bit dimensions, 14-bit entries, short lengths, 1-bit lookup type)
/// widened to a standard one.
fn rebuild_book(cb: &[u8], id: u32, os: &mut BitWriter) -> Result<(), String> {
    let mut bis = BitReader::new(cb, 0);
    let dims = bis.bits(4)?;
    let entries = bis.bits(14)?;
    os.bits(0x56_4342, 24);
    os.bits(dims, 16);
    os.bits(entries, 24);
    let ordered = bis.bits(1)?;
    os.bits(ordered, 1);
    if ordered != 0 {
        let v = bis.bits(5)?;
        os.bits(v, 5);
        let mut cur = 0u32;
        while cur < entries {
            let w = ilog(entries - cur);
            let number = bis.bits(w)?;
            os.bits(number, w);
            cur += number;
        }
        if cur > entries {
            return Err("ww2ogg: current_entry out of range".into());
        }
    } else {
        let len_bits = bis.bits(3)?;
        let sparse = bis.bits(1)?;
        if len_bits == 0 || len_bits > 5 {
            return Err("ww2ogg: nonsense codeword length".into());
        }
        os.bits(sparse, 1);
        for _ in 0..entries {
            let mut present = 1;
            if sparse != 0 {
                present = bis.bits(1)?;
                os.bits(present, 1);
            }
            if present != 0 {
                let v = bis.bits(len_bits)?;
                os.bits(v, 5);
            }
        }
    }
    let lookup = bis.bits(1)?;
    os.bits(lookup, 4);
    if lookup == 1 {
        for _ in 0..2 {
            let v = bis.bits(32)?;
            os.bits(v, 32);
        }
        let value_length = bis.bits(4)?;
        os.bits(value_length, 4);
        let v = bis.bits(1)?;
        os.bits(v, 1);
        for _ in 0..quantvals(entries, dims)? {
            let v = bis.bits(value_length + 1)?;
            os.bits(v, value_length + 1);
        }
    }
    // every bit of the book used (a book whose bits fill its last byte has one extra 0 byte)
    if bis.total / 8 + 1 != cb.len() {
        return Err(format!("ww2ogg: codebook {id} size mismatch: expected {}, got {}", cb.len(), bis.total / 8 + 1));
    }
    Ok(())
}

/// A wem's Vorbis packets: the identification and setup headers (the comment header is not built) and the audio
/// packets in order, each with the block size it decodes.
pub struct Vorbis {
    pub ident: Vec<u8>,
    pub setup: Vec<u8>,
    pub audio: Vec<Vec<u8>>,
    /// The block size (samples) of each audio packet.
    pub blocks: Vec<u32>,
}

fn head(os: &mut BitWriter, ty: u32) {
    os.bits(ty, 8);
    for c in b"vorbis" {
        os.bits(u32::from(*c), 8);
    }
}

/// The standard Vorbis packets of a wem. Fails, naming `what`, where ww2ogg would.
pub fn wem_to_vorbis(wem: &[u8], books: &CodebookLibrary, what: &str) -> Result<Vorbis, String> {
    convert(wem, books).map_err(|m| format!("ww2ogg: {what}: {}", m.trim_start_matches("ww2ogg: ")))
}

fn convert(wem: &[u8], books: &CodebookLibrary) -> Result<Vorbis, String> {
    use super::wem::{u16_at, u32_at};
    let fail = |m: &str| -> String { m.to_owned() };
    if wem.len() < 12 {
        return Err(fail("missing RIFF"));
    }
    if &wem[0..4] == b"RIFX" {
        return Err(fail("a RIFX (big-endian) wem: not ported"));
    }
    if &wem[0..4] != b"RIFF" {
        return Err(fail("missing RIFF"));
    }
    let riff_size = u32_at(wem, 4) as usize + 8;
    if riff_size > wem.len() {
        return Err(fail("RIFF truncated"));
    }
    if &wem[8..12] != b"WAVE" {
        return Err(fail("missing WAVE"));
    }

    let (mut fmt_off, mut fmt_size, mut smpl_off, mut vorb_off, mut data_off, mut data_size) =
        (None, None, None, None, None, 0usize);
    let mut at = 12usize;
    while at < riff_size {
        if at + 8 > riff_size {
            return Err(fail("chunk header truncated"));
        }
        let id = &wem[at..at + 4];
        let size = u32_at(wem, at + 4) as usize;
        match id {
            b"fmt " => {
                fmt_off = Some(at + 8);
                fmt_size = Some(size);
            }
            b"smpl" => smpl_off = Some(at + 8),
            b"vorb" => vorb_off = Some(at + 8),
            b"data" => {
                data_off = Some(at + 8);
                data_size = size;
            }
            _ => {}
        }
        at += 8 + size;
    }
    if at > riff_size {
        return Err(fail("chunk truncated"));
    }
    if fmt_off.is_none() && data_off.is_none() {
        return Err(fail("expected fmt, data chunks"));
    }
    if vorb_off.is_some() {
        return Err(fail("a separate 'vorb' chunk: not ported"));
    }
    let (Some(fmt_off), Some(fmt_size)) = (fmt_off, fmt_size) else {
        return Err(fail("expected 0x42 fmt if vorb missing"));
    };
    if fmt_size != 0x42 {
        return Err(fail("expected 0x42 fmt if vorb missing"));
    }
    let Some(data_off) = data_off else { return Err(fail("no 'data' chunk")) };
    let vorb_off = fmt_off + 0x18;

    if u16_at(wem, fmt_off) != 0xffff {
        return Err(fail("bad codec id"));
    }
    let channels = u32::from(u16_at(wem, fmt_off + 2));
    let rate = u32_at(wem, fmt_off + 4);
    let avg_bytes = u32_at(wem, fmt_off + 8);
    if u16_at(wem, fmt_off + 12) != 0 {
        return Err(fail("bad block align"));
    }
    if u16_at(wem, fmt_off + 14) != 0 {
        return Err(fail("expected 0 bps"));
    }
    if usize::from(u16_at(wem, fmt_off + 16)) != fmt_size - 0x12 {
        return Err(fail("bad extra fmt length"));
    }

    let (mut loop_count, mut loop_start, mut loop_end) = (0, 0, 0);
    if let Some(s) = smpl_off {
        loop_count = u32_at(wem, s + 0x1c);
        if loop_count != 1 {
            return Err(fail("expected one loop"));
        }
        loop_start = u32_at(wem, s + 0x2c);
        loop_end = u32_at(wem, s + 0x30);
    }

    let sample_count = u32_at(wem, vorb_off);
    let mod_signal = u32_at(wem, vorb_off + 0x04);
    // ww2ogg's mod_packets test: 0x4A, 0x4B, 0x69 and 0x70 mean standard packets that carry their own type and window
    // bits, a layout no medium of this build has (every one is mod_packets), so it is not ported
    if matches!(mod_signal, 0x4a | 0x4b | 0x69 | 0x70) {
        return Err(format!("standard Vorbis packets (vorb +0x04 is 0x{mod_signal:x}): not a layout of this build"));
    }
    let setup_offset = u32_at(wem, vorb_off + 0x10) as usize;
    let first_audio = u32_at(wem, vorb_off + 0x14) as usize;
    let bs0 = u32::from(wem[vorb_off + 0x28]);
    let bs1 = u32::from(wem[vorb_off + 0x29]);

    if loop_count != 0 {
        loop_end = if loop_end == 0 { sample_count } else { loop_end.wrapping_add(1) };
        if loop_start >= sample_count || loop_end > sample_count || loop_start > loop_end {
            return Err(fail("loops out of range"));
        }
    }

    // identification
    let ident = {
        let mut os = BitWriter::new(30);
        head(&mut os, 1);
        os.bits(0, 32);
        os.bits(channels, 8);
        os.bits(rate, 32);
        os.bits(0, 32);
        os.bits(avg_bytes.wrapping_mul(8), 32);
        os.bits(0, 32);
        os.bits(bs0, 4);
        os.bits(bs1, 4);
        os.bits(1, 1);
        os.end()?
    };

    // setup
    let mut mode_blockflag: Vec<bool> = Vec::new();
    let mode_bits;
    let setup = {
        let mut os = BitWriter::new(8192);
        head(&mut os, 5);
        let p_off = data_off + setup_offset;
        if p_off + 2 > wem.len() {
            return Err(fail("setup packet header past the file"));
        }
        let setup_size = usize::from(u16_at(wem, p_off));
        let mut ss = BitReader::new(wem, p_off + 2);
        let count = ss.bits(8)? + 1;
        os.bits(count - 1, 8);
        for _ in 0..count {
            let id = ss.bits(10)?;
            let Some(cb) = books.book(id as usize) else { return Err(format!("invalid codebook id {id}")) };
            rebuild_book(cb, id, &mut os)?;
        }
        // time domain transforms (placeholder)
        os.bits(0, 6);
        os.bits(0, 16);

        let floor_count = ss.bits(6)? + 1;
        os.bits(floor_count - 1, 6);
        for _ in 0..floor_count {
            os.bits(1, 16); // floor type 1
            let partitions = ss.bits(5)?;
            os.bits(partitions, 5);
            let mut class_list = Vec::new();
            let mut max_class = 0;
            for _ in 0..partitions {
                let c = ss.bits(4)?;
                os.bits(c, 4);
                class_list.push(c);
                max_class = max_class.max(c);
            }
            let mut dims = Vec::new();
            for _ in 0..=max_class {
                let d = ss.bits(3)?;
                os.bits(d, 3);
                dims.push(d + 1);
                let sub = ss.bits(2)?;
                os.bits(sub, 2);
                if sub != 0 {
                    let master = ss.bits(8)?;
                    os.bits(master, 8);
                    if master >= count {
                        return Err(fail("invalid floor1 masterbook"));
                    }
                }
                for _ in 0..1u32 << sub {
                    let b = ss.bits(8)?;
                    os.bits(b, 8);
                    if b > count {
                        return Err(fail("invalid floor1 subclass book"));
                    }
                }
            }
            let v = ss.bits(2)?; // multiplier - 1
            os.bits(v, 2);
            let rangebits = ss.bits(4)?;
            os.bits(rangebits, 4);
            for &c in &class_list {
                for _ in 0..dims[c as usize] {
                    let v = ss.bits(rangebits)?;
                    os.bits(v, rangebits);
                }
            }
        }

        let residue_count = ss.bits(6)? + 1;
        os.bits(residue_count - 1, 6);
        for _ in 0..residue_count {
            let ty = ss.bits(2)?;
            os.bits(ty, 16);
            if ty > 2 {
                return Err(fail("invalid residue type"));
            }
            let begin = ss.bits(24)?;
            let end = ss.bits(24)?;
            let psize = ss.bits(24)?;
            let classif = ss.bits(6)? + 1;
            let classbook = ss.bits(8)?;
            os.bits(begin, 24);
            os.bits(end, 24);
            os.bits(psize, 24);
            os.bits(classif - 1, 6);
            os.bits(classbook, 8);
            if classbook >= count {
                return Err(fail("invalid residue classbook"));
            }
            let mut cascade = Vec::new();
            for _ in 0..classif {
                let low = ss.bits(3)?;
                os.bits(low, 3);
                let flag = ss.bits(1)?;
                os.bits(flag, 1);
                let mut high = 0;
                if flag != 0 {
                    high = ss.bits(5)?;
                    os.bits(high, 5);
                }
                cascade.push(high * 8 + low);
            }
            for &c in &cascade {
                for k in 0..8 {
                    if c & (1 << k) != 0 {
                        let book = ss.bits(8)?;
                        os.bits(book, 8);
                        if book >= count {
                            return Err(fail("invalid residue book"));
                        }
                    }
                }
            }
        }

        let mapping_count = ss.bits(6)? + 1;
        os.bits(mapping_count - 1, 6);
        for _ in 0..mapping_count {
            os.bits(0, 16); // mapping type 0
            let submaps_flag = ss.bits(1)?;
            os.bits(submaps_flag, 1);
            let mut submaps = 1;
            if submaps_flag != 0 {
                let s = ss.bits(4)?;
                submaps = s + 1;
                os.bits(s, 4);
            }
            let square_polar = ss.bits(1)?;
            os.bits(square_polar, 1);
            if square_polar != 0 {
                let steps = ss.bits(8)? + 1;
                os.bits(steps - 1, 8);
                let w = ilog(channels.wrapping_sub(1));
                for _ in 0..steps {
                    let magnitude = ss.bits(w)?;
                    let angle = ss.bits(w)?;
                    os.bits(magnitude, w);
                    os.bits(angle, w);
                    if angle == magnitude || magnitude >= channels || angle >= channels {
                        return Err(fail("invalid coupling"));
                    }
                }
            }
            let reserved = ss.bits(2)?;
            os.bits(reserved, 2);
            if reserved != 0 {
                return Err(fail("mapping reserved field nonzero"));
            }
            if submaps > 1 {
                for _ in 0..channels {
                    let mux = ss.bits(4)?;
                    os.bits(mux, 4);
                    if mux >= submaps {
                        return Err(fail("mapping_mux >= submaps"));
                    }
                }
            }
            for _ in 0..submaps {
                let v = ss.bits(8)?; // time config
                os.bits(v, 8);
                let floor = ss.bits(8)?;
                os.bits(floor, 8);
                if floor >= floor_count {
                    return Err(fail("invalid floor mapping"));
                }
                let residue = ss.bits(8)?;
                os.bits(residue, 8);
                if residue >= residue_count {
                    return Err(fail("invalid residue mapping"));
                }
            }
        }

        let mode_count = ss.bits(6)? + 1;
        os.bits(mode_count - 1, 6);
        mode_bits = ilog(mode_count - 1);
        for _ in 0..mode_count {
            let flag = ss.bits(1)?;
            os.bits(flag, 1);
            mode_blockflag.push(flag != 0);
            os.bits(0, 16); // window type
            os.bits(0, 16); // transform type
            let mapping = ss.bits(8)?;
            os.bits(mapping, 8);
            if mapping >= mapping_count {
                return Err(fail("invalid mode mapping"));
            }
        }
        os.bits(1, 1);
        let packet = os.end()?;

        if ss.total.div_ceil(8) != setup_size {
            return Err(fail("didn't read exactly setup packet"));
        }
        if p_off + 2 + setup_size != data_off + first_audio {
            return Err(fail("first audio packet doesn't follow setup packet"));
        }
        packet
    };

    // audio packets
    let data_end = data_off + data_size;
    let mut off = data_off + first_audio;
    let mut prev_blockflag = false;
    let mut audio = Vec::new();
    let mut blocks = Vec::new();
    let mode_mask = (1u32 << mode_bits) - 1;
    while off < data_end {
        if off + 2 > data_end {
            return Err(fail("page header truncated"));
        }
        let size = usize::from(u16_at(wem, off));
        let payload = off + 2;
        let next = payload + size;
        if payload >= wem.len() {
            return Err(fail("file truncated"));
        }
        if payload + size > wem.len() {
            return Err(fail("file truncated"));
        }
        let mut os = BitWriter::new(size + 2);
        os.bits(0, 1); // packet type: audio
        let first = u32::from(wem[payload]);
        let mode = first & mode_mask;
        let Some(&flag) = mode_blockflag.get(mode as usize) else {
            return Err(format!("mode {mode} of {}", mode_blockflag.len()));
        };
        os.bits(mode, mode_bits);
        if flag {
            // long window: peek at the next packet's mode
            let mut next_flag = false;
            if next + 2 <= data_end {
                let nsize = u16_at(wem, next);
                if nsize > 0 {
                    if next + 2 >= wem.len() {
                        return Err(fail("out of bits"));
                    }
                    let nmode = u32::from(wem[next + 2]) & mode_mask;
                    next_flag = *mode_blockflag
                        .get(nmode as usize)
                        .ok_or_else(|| format!("mode {nmode} of {}", mode_blockflag.len()))?;
                }
            }
            os.bits(u32::from(prev_blockflag), 1);
            os.bits(u32::from(next_flag), 1);
        }
        prev_blockflag = flag;
        os.bits(first >> mode_bits, 8 - mode_bits);
        os.bytes(&wem[payload + 1..payload + size.max(1)]);
        audio.push(os.end()?);
        blocks.push(1u32 << if flag { bs1 } else { bs0 });
        off = next;
    }
    if off > data_end {
        return Err(fail("page truncated"));
    }
    Ok(Vorbis { ident, setup, audio, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_round_trip_and_unaligned_bytes() {
        let mut w = BitWriter::new(8);
        w.bits(0b101, 3);
        w.bytes(&[0xff, 0x01]);
        w.bits(0x1ff, 9);
        let out = w.end().unwrap();
        let mut r = BitReader::new(&out, 0);
        assert_eq!(r.bits(3).unwrap(), 0b101);
        assert_eq!(r.bits(8).unwrap(), 0xff);
        assert_eq!(r.bits(8).unwrap(), 0x01);
        assert_eq!(r.bits(9).unwrap(), 0x1ff);
        assert!(BitReader::new(&[], 0).bit().is_err());
    }

    #[test]
    fn lookup_values_and_ilog() {
        assert_eq!((ilog(0), ilog(1), ilog(255), ilog(256)), (0, 1, 8, 9));
        // quantvals(entries, dims): the largest v with v^dims <= entries
        assert_eq!(quantvals(81, 4).unwrap(), 3);
        assert_eq!(quantvals(1000, 3).unwrap(), 10);
        assert!(quantvals(5, 0).is_err());
    }
}
