//! Zip files as a release uses them: the packing tool's writer (deflated entries, no zip64), and one reader for the
//! guard (in memory) and the updater (to files): its central directory read with Zip64, each entry stored or
//! deflated and checked against its CRC-32. Encrypted entries and other methods are refused.

use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const END_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const ZIP64_END: u32 = 0x0606_4b50;
/// General purpose flag bit 11: the name is UTF-8.
const UTF8_NAME: u16 = 1 << 11;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;

/// One file of a zip: its name (`/` between folders) and its bytes; `executable` marks a Unix program (the Linux
/// relay), which keeps its mode where the unpacker reads Unix modes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// A name's folder and file parts, refused when it is not a plain relative path inside the zip's folder: a leading
/// slash, a `.` or `..` part, a drive (`:`), or a part ending in `.` or a space (Windows drops those, so `data.` would
/// land on `data`).
pub fn parts(name: &str) -> Result<Vec<&str>, String> {
    let bad = || format!("zip: {name} is not a relative path inside the folder");
    if name.starts_with(['/', '\\']) {
        return Err(bad());
    }
    let parts: Vec<&str> = name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|s| *s == "." || *s == ".." || s.contains(':') || s.ends_with(['.', ' '])) {
        return Err(bad());
    }
    Ok(parts)
}

// ---------------------------------------------------------------------------------------------------------------
// The writer

/// The DOS time and date of now, local time as a zip stores it (unpackers read it as their local time).
#[cfg(windows)]
fn dos_now() -> (u16, u16) {
    // SAFETY: GetLocalTime only writes the SYSTEMTIME it returns.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let y = t.wYear.clamp(1980, 2107);
    ((t.wHour << 11) | (t.wMinute << 5) | (t.wSecond / 2), ((y - 1980) << 9) | (t.wMonth << 5) | t.wDay)
}

/// Elsewhere (no local time read): 1980-01-01, the first DOS date.
#[cfg(not(windows))]
fn dos_now() -> (u16, u16) {
    (0, 1 << 5 | 1)
}

/// A size or offset below 0xFFFFFFFF, the zip64 marker.
fn below_zip64(v: usize) -> Option<u32> {
    u32::try_from(v).ok().filter(|&v| v != u32::MAX)
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A zip of `entries`, each deflated at the best level.
pub fn write(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let too_big = |what: &str| format!("zip: {what} is past 4 GiB (no zip64 here)");
    let (time, date) = dos_now();
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        if e.name.contains('\\') {
            return Err(format!("zip: {} has a backslash", e.name));
        }
        parts(&e.name)?;
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&e.data).map_err(|err| format!("zip: {}: {err}", e.name))?;
        let packed = enc.finish().map_err(|err| format!("zip: {}: {err}", e.name))?;
        let mut crc = flate2::Crc::new();
        crc.update(&e.data);
        let size = below_zip64(e.data.len()).ok_or_else(|| too_big(&e.name))?;
        let csize = below_zip64(packed.len()).ok_or_else(|| too_big(&e.name))?;
        let offset = below_zip64(out.len()).ok_or_else(|| too_big("the zip"))?;
        let name_len = u16::try_from(e.name.len()).map_err(|_| format!("zip: {} is too long a name", e.name))?;

        u32le(&mut out, LOCAL_SIG);
        u16le(&mut out, 20);
        u16le(&mut out, UTF8_NAME);
        u16le(&mut out, DEFLATED);
        u16le(&mut out, time);
        u16le(&mut out, date);
        u32le(&mut out, crc.sum());
        u32le(&mut out, csize);
        u32le(&mut out, size);
        u16le(&mut out, name_len);
        u16le(&mut out, 0);
        out.extend_from_slice(e.name.as_bytes());
        out.extend_from_slice(&packed);

        // made by Unix (3), zip 2.0: the external attributes carry the Unix mode in their high half
        let mode: u32 = if e.executable { 0o100_755 } else { 0o100_644 };
        u32le(&mut central, CENTRAL_SIG);
        u16le(&mut central, 3 << 8 | 20);
        u16le(&mut central, 20);
        u16le(&mut central, UTF8_NAME);
        u16le(&mut central, DEFLATED);
        u16le(&mut central, time);
        u16le(&mut central, date);
        u32le(&mut central, crc.sum());
        u32le(&mut central, csize);
        u32le(&mut central, size);
        u16le(&mut central, name_len);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, mode << 16);
        u32le(&mut central, offset);
        central.extend_from_slice(e.name.as_bytes());
    }
    let count = u16::try_from(entries.len())
        .ok()
        .filter(|&c| c != u16::MAX)
        .ok_or_else(|| "zip: 65535 files or more (no zip64 here)".to_owned())?;
    let cd_offset = below_zip64(out.len()).ok_or_else(|| too_big("the zip"))?;
    let cd_size = below_zip64(central.len()).ok_or_else(|| too_big("the zip's directory"))?;
    out.extend_from_slice(&central);
    u32le(&mut out, END_SIG);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, count);
    u16le(&mut out, count);
    u32le(&mut out, cd_size);
    u32le(&mut out, cd_offset);
    u16le(&mut out, 0);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------
// The reader

/// An entry of the central directory.
#[derive(Debug)]
struct Item {
    name: String,
    method: u16,
    crc: u32,
    packed: u64,
    size: u64,
    /// Its local header's offset.
    at: u64,
    executable: bool,
}

fn u16_at(b: &[u8], k: usize) -> u16 {
    u16::from_le_bytes([b[k], b[k + 1]])
}
fn u32_at(b: &[u8], k: usize) -> u32 {
    u32::from_le_bytes(b[k..k + 4].try_into().expect("four bytes"))
}
fn u64_at(b: &[u8], k: usize) -> u64 {
    u64::from_le_bytes(b[k..k + 8].try_into().expect("eight bytes"))
}

fn io(e: std::io::Error) -> String {
    format!("zip: {e}")
}

/// The central directory's entries.
fn directory<R: Read + Seek>(f: &mut R) -> Result<Vec<Item>, String> {
    let len = f.seek(SeekFrom::End(0)).map_err(io)?;
    // (the end record is 22 bytes and a comment of at most 65535 after it; the Zip64 locator 20 before it)
    let tail_len = len.min(22 + 65_535 + 20);
    f.seek(SeekFrom::Start(len - tail_len)).map_err(io)?;
    let mut tail = vec![0u8; usize::try_from(tail_len).expect("under 64 KiB")];
    f.read_exact(&mut tail).map_err(io)?;
    let e = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&k| u32_at(&tail, k) == END_SIG)
        .ok_or_else(|| "zip: no end record".to_owned())?;
    let mut count = u64::from(u16_at(&tail, e + 10));
    let mut cd_size = u64::from(u32_at(&tail, e + 12));
    let mut cd_at = u64::from(u32_at(&tail, e + 16));
    if (count == 0xffff || cd_size == 0xffff_ffff || cd_at == 0xffff_ffff) && e >= 20 {
        let l = e - 20;
        if u32_at(&tail, l) == ZIP64_LOCATOR {
            f.seek(SeekFrom::Start(u64_at(&tail, l + 8))).map_err(io)?;
            let mut z = [0u8; 56];
            f.read_exact(&mut z).map_err(io)?;
            if u32_at(&z, 0) != ZIP64_END {
                return Err("zip: its Zip64 end record does not read".to_owned());
            }
            count = u64_at(&z, 32);
            cd_size = u64_at(&z, 40);
            cd_at = u64_at(&z, 48);
        }
    }
    if cd_at.checked_add(cd_size).is_none_or(|end| end > len) {
        return Err("zip: its central directory lies past its end".to_owned());
    }
    f.seek(SeekFrom::Start(cd_at)).map_err(io)?;
    let mut cd = vec![0u8; usize::try_from(cd_size).map_err(|_| "zip: its central directory is too large")?];
    f.read_exact(&mut cd).map_err(io)?;
    let mut out = Vec::new();
    let mut k = 0;
    for _ in 0..count {
        if k + 46 > cd.len() || u32_at(&cd, k) != CENTRAL_SIG {
            return Err("zip: a damaged central directory".to_owned());
        }
        let flags = u16_at(&cd, k + 8);
        let method = u16_at(&cd, k + 10);
        let crc = u32_at(&cd, k + 16);
        let mut packed = u64::from(u32_at(&cd, k + 20));
        let mut size = u64::from(u32_at(&cd, k + 24));
        let (n, x, c) =
            (usize::from(u16_at(&cd, k + 28)), usize::from(u16_at(&cd, k + 30)), usize::from(u16_at(&cd, k + 32)));
        let attrs = u32_at(&cd, k + 38);
        let mut at = u64::from(u32_at(&cd, k + 42));
        let end = k + 46 + n + x + c;
        if end > cd.len() {
            return Err("zip: a damaged central directory".to_owned());
        }
        let name = std::str::from_utf8(&cd[k + 46..k + 46 + n]).map_err(|_| "zip: an entry's name is not UTF-8")?;
        if flags & 1 != 0 {
            return Err(format!("zip: {name} is encrypted"));
        }
        // The Zip64 extra field: the 8-byte values of the fields that hold 0xffffffff, in this order.
        let mut ex = &cd[k + 46 + n..k + 46 + n + x];
        while ex.len() >= 4 {
            let (id, sz) = (u16_at(ex, 0), usize::from(u16_at(ex, 2)));
            let body = ex.get(4..4 + sz).ok_or_else(|| format!("zip: {name}: an extra field does not read"))?;
            if id == 1 {
                let mut p = 0;
                for v in [&mut size, &mut packed, &mut at] {
                    if *v == 0xffff_ffff {
                        *v = u64_at(body.get(p..p + 8).ok_or_else(|| format!("zip: {name}: its Zip64 field"))?, 0);
                        p += 8;
                    }
                }
            }
            ex = &ex[4 + sz..];
        }
        if [size, packed, at].contains(&0xffff_ffff) {
            return Err(format!("zip: {name}: a Zip64 marker with no Zip64 field"));
        }
        out.push(Item { name: name.to_owned(), method, crc, packed, size, at, executable: (attrs >> 16) & 0o111 != 0 });
        k = end;
    }
    Ok(out)
}

/// Copies the entry `it` of `f` to `out`, checking its size and CRC-32.
fn extract<R: Read + Seek>(f: &mut R, it: &Item, out: &mut dyn Write) -> Result<(), String> {
    f.seek(SeekFrom::Start(it.at)).map_err(io)?;
    let mut h = [0u8; 30];
    f.read_exact(&mut h).map_err(io)?;
    if u32_at(&h, 0) != LOCAL_SIG {
        return Err(format!("zip: {}: a damaged local header", it.name));
    }
    let skip = u64::from(u16_at(&h, 26)) + u64::from(u16_at(&h, 28));
    f.seek(SeekFrom::Start(it.at + 30 + skip)).map_err(io)?;
    let raw = f.take(it.packed);
    let mut src: Box<dyn Read + '_> = match it.method {
        STORED => Box::new(raw),
        DEFLATED => Box::new(DeflateDecoder::new(raw)),
        m => return Err(format!("zip: {}: method {m} is not read here", it.name)),
    };
    let mut crc = flate2::Crc::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        let n = src.read(&mut buf).map_err(|e| format!("zip: {}: {e}", it.name))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        if got > it.size {
            return Err(format!("zip: {}: longer than its size", it.name));
        }
        crc.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(io)?;
    }
    if got != it.size || crc.sum() != it.crc {
        return Err(format!("zip: {}: its size or CRC does not match", it.name));
    }
    Ok(())
}

/// Every entry of a zip in memory (folders included, their names ending in `/`), unpacked and checked.
pub fn read(zip: &[u8]) -> Result<Vec<Entry>, String> {
    let mut f = Cursor::new(zip);
    let items = directory(&mut f)?;
    let mut out = Vec::with_capacity(items.len());
    for it in &items {
        let mut data = Vec::with_capacity(usize::try_from(it.size).unwrap_or(0).min(zip.len() * 8));
        extract(&mut f, it, &mut data)?;
        out.push(Entry { name: it.name.clone(), data, executable: it.executable });
    }
    Ok(out)
}

/// Unpacks the zip file `zip` into the folder `into` (made when missing); a name that `parts` refuses stops it.
pub fn unpack(zip: &Path, into: &Path) -> Result<(), String> {
    let mut f = BufReader::new(File::open(zip).map_err(|e| format!("zip: {}: {e}", zip.display()))?);
    let items = directory(&mut f)?;
    std::fs::create_dir_all(into).map_err(io)?;
    for it in &items {
        let mut path = into.to_path_buf();
        path.extend(parts(&it.name)?);
        if it.name.ends_with(['/', '\\']) {
            std::fs::create_dir_all(&path).map_err(io)?;
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let mut out = BufWriter::new(File::create(&path).map_err(|e| format!("zip: {}: {e}", path.display()))?);
        extract(&mut f, it, &mut out)?;
        out.flush().map_err(io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_reads_back() {
        let entries = vec![
            Entry { name: "mvs.exe".into(), data: vec![7; 100_000], executable: false },
            Entry { name: "licenses/a.txt".into(), data: b"text".to_vec(), executable: false },
            Entry { name: "mvs-relay".into(), data: Vec::new(), executable: true },
        ];
        let zip = write(&entries).unwrap();
        assert_eq!(read(&zip).unwrap(), entries);
        assert!(write(&[Entry { name: "../x".into(), data: Vec::new(), executable: false }]).is_err());
        let mut bad = zip.clone();
        bad[40] ^= 0xff;
        assert!(read(&bad).is_err());
        // a zip64 marker in the first central entry's compressed size, with no Zip64 field
        let cd = zip.windows(4).position(|w| w == CENTRAL_SIG.to_le_bytes()).unwrap();
        let mut z64 = zip.clone();
        z64[cd + 20..cd + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read(&z64).err().unwrap(), "zip: mvs.exe: a Zip64 marker with no Zip64 field");
    }

    #[test]
    fn names_stay_inside() {
        assert_eq!(parts("a/b.txt").unwrap(), ["a", "b.txt"]);
        assert_eq!(parts("a\\b.txt").unwrap(), ["a", "b.txt"]);
        assert_eq!(parts("licenses/").unwrap(), ["licenses"]);
        for bad in ["../b.txt", "a/../../b.txt", "/b.txt", "C:/b.txt", "", "data./x", "data /x", "a/b."] {
            assert!(parts(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn zip64_fields_read() {
        // one stored entry whose central sizes and offset sit in a Zip64 extra field, and a Zip64 end record
        let data = b"zip64 body";
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let mut z = Vec::new();
        u32le(&mut z, LOCAL_SIG);
        z.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        u32le(&mut z, crc.sum());
        u32le(&mut z, data.len() as u32);
        u32le(&mut z, data.len() as u32);
        u16le(&mut z, 1);
        u16le(&mut z, 0);
        z.push(b'a');
        z.extend_from_slice(data);
        let cd_at = z.len();
        u32le(&mut z, CENTRAL_SIG);
        z.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        u32le(&mut z, crc.sum());
        u32le(&mut z, u32::MAX);
        u32le(&mut z, u32::MAX);
        u16le(&mut z, 1);
        u16le(&mut z, 4 + 24);
        z.extend_from_slice(&[0; 6]);
        u32le(&mut z, 0);
        u32le(&mut z, u32::MAX);
        z.push(b'a');
        u16le(&mut z, 1);
        u16le(&mut z, 24);
        z.extend_from_slice(&(data.len() as u64).to_le_bytes());
        z.extend_from_slice(&(data.len() as u64).to_le_bytes());
        z.extend_from_slice(&0u64.to_le_bytes());
        let cd_size = z.len() - cd_at;
        let end64 = z.len();
        u32le(&mut z, ZIP64_END);
        z.extend_from_slice(&44u64.to_le_bytes());
        z.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&1u64.to_le_bytes());
        z.extend_from_slice(&1u64.to_le_bytes());
        z.extend_from_slice(&(cd_size as u64).to_le_bytes());
        z.extend_from_slice(&(cd_at as u64).to_le_bytes());
        u32le(&mut z, ZIP64_LOCATOR);
        u32le(&mut z, 0);
        z.extend_from_slice(&(end64 as u64).to_le_bytes());
        u32le(&mut z, 1);
        u32le(&mut z, END_SIG);
        z.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        u32le(&mut z, u32::MAX);
        u32le(&mut z, u32::MAX);
        u16le(&mut z, 0);
        let back = read(&z).unwrap();
        assert_eq!((back[0].name.as_str(), back[0].data.as_slice()), ("a", &data[..]));
    }
}
