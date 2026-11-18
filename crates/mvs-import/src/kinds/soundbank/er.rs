//! AkRoomVerb's early reflection patterns: `AkRoomVerb::g_ERPatternsList` of the depot's AkRoomVerb.dll (VA
//! 0x1800259b0), the one place the build holds them. Read as 31 entries of 0x28 bytes `{name, uNumTapsLeft +8,
//! TapInfoLeft* +0x10, uNumTapsRight +0x18, TapInfoRight* +0x20}`, a TapInfo `{float time ms, float gain}`; the VAs
//! are mapped to the file through the PE section table. The table the build list hashes is the 31 entries' tap
//! counts followed by every tap array, left then right, in entry order (the pointers and names are left out: they
//! place the data, they are not it).

use mvs_data::sound::ErTaps;

/// The DLL, relative to the build's folder.
pub(super) const ROOMVERB_DLL: &str = "MultiVersus/Plugins/Wwise/ThirdParty/x64_vc160/Release/bin/AkRoomVerb.dll";
/// The table's name in the build list.
pub(super) const TABLE: &str = "erPatterns";

const LIST_VA: u64 = 0x1_8002_59b0;
const ENTRIES: usize = 31;
const ENTRY_SIZE: usize = 0x28;

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

struct Section {
    va: u64,
    vsize: u64,
    raw: u64,
    rsize: u64,
}

/// The pattern list of the DLL's bytes, and the table bytes the build list hashes. An error for a file that is not
/// a PE image holding the list where the build's does.
pub(super) fn read_er_patterns(dll: &[u8]) -> Result<(Vec<ErTaps>, Vec<u8>), String> {
    let bad = |m: String| format!("erpatterns: {m}");
    if dll.len() < 0x40 || u16_at(dll, 0) != Some(0x5a4d) {
        return Err(bad("AkRoomVerb.dll is not a PE image".into()));
    }
    let pe = u32_at(dll, 0x3c).ok_or_else(|| bad("truncated".into()))? as usize;
    if u32_at(dll, pe) != Some(0x4550) || u16_at(dll, pe + 24) != Some(0x20b) {
        return Err(bad("AkRoomVerb.dll is not a PE32+ image".into()));
    }
    let trunc = || bad("truncated".into());
    let nsec = u16_at(dll, pe + 6).ok_or_else(trunc)? as usize;
    let opt_size = u16_at(dll, pe + 20).ok_or_else(trunc)? as usize;
    let base = u64_at(dll, pe + 48).ok_or_else(trunc)?;
    let mut secs = Vec::new();
    for i in 0..nsec {
        let s = pe + 24 + opt_size + i * 40;
        secs.push(Section {
            vsize: u64::from(u32_at(dll, s + 8).ok_or_else(trunc)?),
            va: u64::from(u32_at(dll, s + 12).ok_or_else(trunc)?),
            rsize: u64::from(u32_at(dll, s + 16).ok_or_else(trunc)?),
            raw: u64::from(u32_at(dll, s + 20).ok_or_else(trunc)?),
        });
    }
    let off = |va: u64, size: usize| -> Result<usize, String> {
        let rva = va.wrapping_sub(base);
        let Some(s) = secs.iter().find(|x| x.va <= rva && rva < x.va + x.vsize.max(x.rsize)) else {
            return Err(bad(format!("VA {va:x} is in no section")));
        };
        let o = s.raw + rva - s.va;
        if rva + size as u64 > s.va + s.rsize || o + size as u64 > dll.len() as u64 {
            return Err(bad(format!("VA {va:x} ({size} bytes) is not in the file")));
        }
        Ok(o as usize)
    };
    let list = off(LIST_VA, ENTRIES * ENTRY_SIZE)?;
    let mut patterns = Vec::new();
    let mut counts = Vec::new();
    let mut taps: Vec<u8> = Vec::new();
    let mut read = |va: u64, n: usize| -> Result<Vec<[f64; 2]>, String> {
        if n > 4096 {
            return Err(bad(format!("{n} taps")));
        }
        let o = off(va, n * 8)?;
        taps.extend_from_slice(&dll[o..o + n * 8]);
        let mut out = Vec::new();
        for j in 0..n {
            let f = |k: usize| f64::from(f32::from_bits(u32_at(dll, o + j * 8 + k).unwrap_or(0)));
            out.push([f(0), f(4)]);
        }
        Ok(out)
    };
    for i in 0..ENTRIES {
        let e = list + i * ENTRY_SIZE;
        let nl = u32_at(dll, e + 8).ok_or_else(trunc)?;
        let nr = u32_at(dll, e + 0x18).ok_or_else(trunc)?;
        counts.extend_from_slice(&nl.to_le_bytes());
        counts.extend_from_slice(&nr.to_le_bytes());
        let left = read(u64_at(dll, e + 0x10).ok_or_else(trunc)?, nl as usize)?;
        let right = read(u64_at(dll, e + 0x20).ok_or_else(trunc)?, nr as usize)?;
        patterns.push(ErTaps { left, right });
    }
    counts.extend_from_slice(&taps);
    Ok((patterns, counts))
}
