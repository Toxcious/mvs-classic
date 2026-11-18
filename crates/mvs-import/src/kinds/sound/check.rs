//! Checks of the sound kind against the oracle's store and vgmstream: the player's paks and the oracle's store are the
//! developer's, so each runs only when asked (`cargo test -p mvs-import --release sound::check -- --ignored`).
//!
//! - `media_equal_oracle`: every medium of a built pack (`MVS_SOUND_OURS`, from `mvs-import kind sound --out`) against
//!   the oracle store's (`MVS_SOUND_ORACLE`, its `sound` Ogg entries): the packets byte for byte, the identification
//!   and setup headers, the rate, channels, length, loop points and per-page granules.
//! - `export_wems` / `vgmstream_diff`: about 200 media written as wems to `MVS_SOUND_WEMS`, decoded by vgmstream
//!   (`MVS_VGMSTREAM`) and through lewton from the pack, compared sample for sample.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
use lewton::header::{read_header_ident, read_header_setup};
use lewton::samples::InterleavedSamples;
use mvs_data::sound::{Medium, decode_medium};
use mvs_pak::{BuildPaths, pak_files_in, setup};
use mvs_store::{Pack, Store};
use mvs_uasset::PackageCache;
use mvs_uasset::package::FileSource;
use rayon::prelude::*;

use super::{SETUP_PREFIX, read_wem};
use crate::catalog::scan_catalog;
use crate::kind::{Cx, Shared};
use crate::reads::StoreReads;

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")))
}

/// An Ogg stream's packets, and per page the index of the last packet it completes and its granule position.
fn ogg(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<(usize, i64)>) {
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut pages = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut open = false;
    let mut at = 0;
    while at < bytes.len() {
        assert_eq!(&bytes[at..at + 4], b"OggS", "a page at {at}");
        let gran = i64::from_le_bytes(bytes[at + 6..at + 14].try_into().unwrap());
        let nseg = usize::from(bytes[at + 26]);
        let lacing = &bytes[at + 27..at + 27 + nseg];
        let mut p = at + 27 + nseg;
        for &l in lacing {
            cur.extend_from_slice(&bytes[p..p + usize::from(l)]);
            p += usize::from(l);
            open = true;
            if l < 255 {
                packets.push(std::mem::take(&mut cur));
                open = false;
            }
        }
        assert!(!open || p == bytes.len() || bytes[p + 5] & 1 != 0, "a packet continues past its page");
        pages.push((packets.len().wrapping_sub(1), gran));
        at = p;
    }
    assert!(!open, "the stream ends inside a packet");
    (packets, pages)
}

fn comment_value(comment: &[u8], key: &str) -> Option<u32> {
    let text = String::from_utf8_lossy(comment);
    let at = text.find(key)? + key.len();
    text[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

fn check_medium(path: &str, ours: &Pack, oracle: &Pack, setups: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let o = oracle.bytes(path).map_err(|e| e.to_string())?;
    let bytes = ours.bytes(path).map_err(|e| format!("ours: {e}"))?;
    let m = decode_medium(&bytes, path).map_err(|e| e.to_string())?;
    let (packets, pages) = ogg(&o);
    if packets.len() != m.packets() + 3 {
        return Err(format!("{} packets, the oracle's {} (+3 headers)", m.packets(), packets.len()));
    }
    if m.header.ident != packets[0] {
        return Err("identification header differs".into());
    }
    let setup = setups.get(&m.header.setup).ok_or_else(|| format!("no setup entry {}", m.header.setup))?;
    if *setup != packets[2] {
        return Err(format!("setup header {} differs", m.header.setup));
    }
    for i in 0..m.packets() {
        if m.packet(i) != packets[i + 3] {
            return Err(format!("audio packet {i} differs"));
        }
    }
    let id = &packets[0];
    let (channels, rate) = (u16::from(id[11]), u32::from_le_bytes(id[12..16].try_into().unwrap()));
    if (channels, rate) != (m.header.channels, m.header.rate) {
        return Err(format!("{channels} ch {rate} Hz in the Ogg, {} ch {} Hz", m.header.channels, m.header.rate));
    }
    let last = pages.last().ok_or("no pages")?.1;
    if last != i64::from(m.header.frames) {
        return Err(format!("{} frames, the Ogg's last granule {last}", m.header.frames));
    }
    // the comment's loop points (an end of 0 was the length; ours always the smpl chunk's end + 1)
    let lp = match (comment_value(&packets[1], "LoopStart="), comment_value(&packets[1], "LoopEnd=")) {
        (Some(s), Some(e)) => Some([s, e]),
        _ => None,
    };
    if lp != m.header.loop_points {
        return Err(format!("loop points {:?}, the Ogg's {lp:?}", m.header.loop_points));
    }
    for &(k, g) in &pages {
        if g >= 0 && k >= 3 && k < packets.len() && g != i64::from(m.granule(k - 3)) {
            return Err(format!("granule {} at packet {}, the page's {g}", m.granule(k - 3), k - 3));
        }
    }
    if m.granule(0) != 0 {
        return Err("the first packet's granule is not 0".into());
    }
    Ok(())
}

#[test]
#[ignore = "needs a built pack and the oracle store"]
fn media_equal_oracle() {
    let ours = Pack::open("sound", env("MVS_SOUND_OURS")).unwrap();
    let store = Store::open(env("MVS_SOUND_ORACLE")).unwrap();
    let oracle = store.kind("sound").unwrap();
    let setups: BTreeMap<String, Vec<u8>> =
        ours.paths().filter(|p| p.starts_with(SETUP_PREFIX)).map(|p| (p.to_owned(), ours.bytes(p).unwrap())).collect();
    let media: Vec<String> = oracle.paths().map(str::to_owned).collect();
    let ours_media = ours.paths().filter(|p| !p.starts_with(SETUP_PREFIX)).count();
    assert_eq!(ours_media, media.len(), "the media listed");
    let bad: Vec<String> = media
        .par_iter()
        .filter_map(|p| check_medium(p, &ours, &oracle, &setups).err().map(|e| format!("{p}: {e}")))
        .collect();
    eprintln!("{} media, {} setups, {} differ", media.len(), setups.len(), bad.len());
    for b in bad.iter().take(20) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}

/// The CLI's way to a context over the developer's paks, run `f` with it.
fn with_cx<T>(f: impl FnOnce(&Cx<'_>) -> T) -> T {
    let dir = env("MVS_PAKS");
    let key = setup::key_from_exe(&setup::exe_beside_paks(&dir)).unwrap();
    let files = pak_files_in(&dir).unwrap();
    let set = Arc::new(mvs_pak::PakSet::open(&files, &key).unwrap());
    let paths = Arc::new(BuildPaths::from_paks(&set).unwrap());
    let cache = PackageCache::new(set.clone() as Arc<dyn FileSource>, paths.clone(), 1 << 30);
    let (catalog, _) = scan_catalog(&set, &paths, &|_| {});
    let shared = Shared::default();
    let reads = StoreReads::none();
    let cx = Cx { paks: &set, paths: &paths, cache: &cache, catalog: &catalog, shared: &shared, store: &reads };
    f(&cx)
}

#[test]
#[ignore = "needs the paks"]
fn export_wems() {
    let ours = Pack::open("sound", env("MVS_SOUND_OURS")).unwrap();
    let out = env("MVS_SOUND_WEMS");
    std::fs::create_dir_all(&out).unwrap();
    let mut media: Vec<&str> = ours.paths().filter(|p| !p.starts_with(SETUP_PREFIX)).collect();
    media.sort_unstable();
    let picked: Vec<&str> = media.iter().step_by(120).copied().collect();
    with_cx(|cx| {
        for (i, p) in picked.iter().enumerate() {
            std::fs::write(out.join(format!("{i:04}.wem")), read_wem(cx, p).unwrap()).unwrap();
            std::fs::write(out.join(format!("{i:04}.txt")), p).unwrap();
        }
    });
    eprintln!("{} wems", picked.len());
}

/// A medium decoded to interleaved f32 and trimmed to its frames.
fn lewton_decode(m: &Medium, setup: &[u8]) -> (u16, Vec<f32>) {
    let ident = read_header_ident(&m.header.ident).unwrap();
    let setup = read_header_setup(setup, ident.audio_channels, (ident.blocksize_0, ident.blocksize_1)).unwrap();
    let mut pwr = PreviousWindowRight::new();
    let mut out = Vec::new();
    for i in 0..m.packets() {
        let s: InterleavedSamples<f32> = read_audio_packet_generic(&ident, &setup, m.packet(i), &mut pwr).unwrap();
        out.extend_from_slice(&s.samples);
    }
    out.truncate(m.header.frames as usize * usize::from(m.header.channels));
    (m.header.channels, out)
}

/// A float WAV's channels and samples.
fn read_wav(b: &[u8]) -> (u16, Vec<f32>) {
    assert_eq!(&b[0..4], b"RIFF");
    let (mut at, mut channels, mut data) = (12, 0u16, Vec::new());
    while at + 8 <= b.len() {
        let size = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..(at + 8 + size).min(b.len())];
        match &b[at..at + 4] {
            b"fmt " => {
                channels = u16::from_le_bytes([body[2], body[3]]);
                assert_eq!(u16::from_le_bytes([body[14], body[15]]), 32, "a 32-bit WAV");
            }
            b"data" => data = body.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    (channels, data)
}

#[test]
#[ignore = "needs vgmstream and the exported wems"]
fn vgmstream_diff() {
    let ours = Pack::open("sound", env("MVS_SOUND_OURS")).unwrap();
    let dir = env("MVS_SOUND_WEMS");
    let exe = env("MVS_VGMSTREAM");
    let mut n = 0;
    let (mut worst, mut worst_at) = (0f32, String::new());
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    for wem in names.iter().filter(|p| p.extension().is_some_and(|e| e == "wem")) {
        let path = std::fs::read_to_string(wem.with_extension("txt")).unwrap();
        let wav = wem.with_extension("wav");
        let st = std::process::Command::new(&exe).args(["-i", "-W", "4", "-o"]).arg(&wav).arg(wem).output().unwrap();
        assert!(st.status.success(), "{}: {}", wem.display(), String::from_utf8_lossy(&st.stderr));
        let (vc, v) = read_wav(&std::fs::read(&wav).unwrap());
        std::fs::remove_file(&wav).unwrap();
        let bytes = ours.bytes(&path).unwrap();
        let m = decode_medium(&bytes, &path).unwrap();
        let setup = ours.bytes(&m.header.setup).unwrap();
        let (c, o) = lewton_decode(&m, &setup);
        assert_eq!((c, o.len()), (vc, v.len()), "{path}: channels and length");
        let d = o.iter().zip(&v).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        if d > worst {
            (worst, worst_at) = (d, path.clone());
        }
        assert!(d < 1e-5, "{path}: max |diff| {d}");
        n += 1;
    }
    eprintln!("{n} media, max |diff| {worst:e} ({worst_at})");
}

#[test]
#[ignore = "needs a built pack and the oracle store"]
fn header_equals_bank_meta() {
    let ours = Pack::open("sound", env("MVS_SOUND_OURS")).unwrap();
    let store = Store::open(env("MVS_SOUND_ORACLE")).unwrap();
    let banks = store.kind("soundbank").unwrap();
    let mut seen: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for p in banks.paths() {
        let bytes = banks.bytes(p).unwrap();
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        if let Some(media) = v.get("media").and_then(|m| m.as_object()) {
            for (_, m) in media {
                seen.insert(m["path"].as_str().unwrap().to_owned(), m.clone());
            }
        }
    }
    assert!(seen.len() > 1000, "{} media in the banks", seen.len());
    let mut bad = Vec::new();
    for (path, m) in &seen {
        let bytes = ours.bytes(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let h = decode_medium(&bytes, path).unwrap().header;
        let num = |k: &str| m.get(k).and_then(serde_json::Value::as_f64);
        let ok = num("samples") == Some(f64::from(h.frames))
            && num("rate") == Some(f64::from(h.rate))
            && num("channels") == Some(f64::from(h.channels))
            && num("mask").unwrap_or(0.0) == f64::from(h.mask)
            && num("loop_start").map(|s| s as u32) == h.loop_points.map(|l| l[0])
            && num("loop_end").map(|s| s as u32) == h.loop_points.map(|l| l[1])
            && num("norm").unwrap_or(0.0) == h.norm;
        if !ok {
            bad.push(format!("{path}: {m} against {h:?}"));
        }
    }
    eprintln!("{} media in the banks, {} differ", seen.len(), bad.len());
    for b in bad.iter().take(10) {
        eprintln!("{b}");
    }
    assert!(bad.is_empty());
}
