//! The packed Vorbis codebook library the setup headers are rebuilt from, read from the build's exe: the sound engine
//! is linked into the exe, whose `g_packedCodebooks` (VA 0x144cef910 in this build) is a 1,024-slot pointer table, 599
//! pointers into the 71,991 bytes of books (VA 0x14459d4a0, the last pointer their end), the rest null. Its only
//! reader, vorbis_unpack_books, takes a book by a 10-bit index into it, so it is the library every Vorbis medium of the
//! build decodes with: ww2ogg's `packed_codebooks_aoTuV_603.bin`.
//!
//! The table is found by its shape, not by an address: the one run of exactly 599 ascending pointers, 8-byte aligned,
//! whose targets lie in one section of the image, with a null after it. What is cut out is written in ww2ogg's .bin
//! layout (the books, then each book's u32 offset and the end's) and checked against the build list's `codebooks`
//! table of the exe.

use mvs_pak::setup;

use super::ww2ogg::CodebookLibrary;

pub const EXE_PATH: &str = "MultiVersus/Binaries/Win64/MultiVersus-Win64-Shipping.exe";
const POINTERS: usize = 599;

struct Section {
    rva: u64,
    vsize: u64,
    raw: u64,
    raw_size: u64,
}

fn u16_at(b: &[u8], o: usize) -> Result<u16, String> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| "codebooks: the exe is truncated".into())
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "codebooks: the exe is truncated".into())
}

fn u64_at(b: &[u8], o: usize) -> Result<u64, String> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().expect("8 bytes")))
        .ok_or_else(|| "codebooks: the exe is truncated".into())
}

/// The library's bytes in ww2ogg's .bin layout, cut from the exe (unchecked). Fails unless exactly one table has the
/// shape.
pub fn cut_codebooks(exe: &[u8]) -> Result<Vec<u8>, String> {
    if u16_at(exe, 0)? != 0x5a4d {
        return Err("codebooks: the exe has no MZ header".into());
    }
    let p = u32_at(exe, 0x3c)? as usize;
    if u32_at(exe, p)? != 0x4550 {
        return Err("codebooks: the exe is not a PE file".into());
    }
    if u16_at(exe, p + 24)? != 0x20b {
        return Err("codebooks: the exe is not PE32+".into());
    }
    let count = usize::from(u16_at(exe, p + 6)?);
    let opt_size = usize::from(u16_at(exe, p + 20)?);
    let base = u64_at(exe, p + 24 + 24)?;
    let image_size = u64::from(u32_at(exe, p + 24 + 56)?);
    let mut sections = Vec::new();
    let mut o = p + 24 + opt_size;
    for _ in 0..count {
        sections.push(Section {
            vsize: u64::from(u32_at(exe, o + 8)?),
            rva: u64::from(u32_at(exe, o + 12)?),
            raw_size: u64::from(u32_at(exe, o + 16)?),
            raw: u64::from(u32_at(exe, o + 20)?),
        });
        o += 40;
    }
    let (lo, hi) = (base, base + image_size);
    let ptr = |o: u64| u64_at(exe, o as usize);
    // the section a pointer's target lies in (its index), None for none
    let section_of = |x: u64| -> Option<usize> {
        if x < lo || x >= hi {
            return None;
        }
        let rva = x - base;
        sections.iter().position(|s| rva >= s.rva && rva < s.rva + s.vsize.min(s.raw_size))
    };
    let mut tables: Vec<u64> = Vec::new();
    for s in &sections {
        let end = (s.raw + s.raw_size.min(s.vsize)) as i64 - 8;
        let (mut run, mut prev, mut in_) = (0usize, 0u64, None::<usize>);
        let mut o = s.raw + (8 - s.raw % 8) % 8;
        while o as i64 <= end {
            let x = ptr(o)?;
            let k = section_of(x);
            if k.is_some() && run > 0 && k == in_ && x > prev {
                run += 1;
            } else {
                if run == POINTERS && x == 0 {
                    tables.push(o - 8 * POINTERS as u64);
                }
                run = usize::from(k.is_some());
                in_ = k;
            }
            prev = x;
            o += 8;
        }
    }
    if tables.len() != 1 {
        return Err(format!("codebooks: {} tables of {POINTERS} ascending pointers in the exe, not one", tables.len()));
    }
    let t = tables[0];
    let first = ptr(t)? - base;
    let Some(sec) = sections.iter().find(|s| first >= s.rva && first < s.rva + s.vsize) else {
        return Err("codebooks: the first book lies in no section".into());
    };
    let at = (first - sec.rva + sec.raw) as usize;
    let size = (ptr(t + 8 * (POINTERS as u64 - 1))? - base - first) as usize;
    let books = exe.get(at..at + size).ok_or("codebooks: the books lie past the exe")?;
    let mut out = Vec::with_capacity(size + 4 * POINTERS);
    out.extend_from_slice(books);
    for i in 0..POINTERS as u64 {
        out.extend_from_slice(&((ptr(t + 8 * i)? - base - first) as u32).to_le_bytes());
    }
    Ok(out)
}

/// The checked library of the exe's bytes (the build list's `codebooks` table).
pub fn exe_codebooks(exe: &[u8]) -> Result<CodebookLibrary, String> {
    let bin = cut_codebooks(exe)?;
    let list = setup::build_list().map_err(|e| e.to_string())?;
    setup::check_table(&list.files, EXE_PATH, "codebooks", &bin).map_err(|e| e.to_string())?;
    CodebookLibrary::from_bin(&bin)
}
