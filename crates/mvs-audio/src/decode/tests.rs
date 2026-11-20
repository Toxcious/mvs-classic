//! The decode against the developer's store (its `sound` kind, `mvs_store::dev_store_dir`): media decoded whole and
//! compared with a decode of their packets written here (lewton's packet calls, each packet's output checked against
//! its granule), and with lewton's own Ogg reader on the oracle's Ogg of the same path (the oracle's store:
//! `MVS_SOUND_ORACLE`; skipped without it); then streamed from seeks (the middle, inside the last packet, the loop
//! start,
//! back to the start). A tree without a store says so and passes.

use std::sync::Arc;

use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
use lewton::header::{read_header_ident, read_header_setup};
use mvs_data::sound::{Medium, decode_medium, sound_pack};

use super::{Coded, Stream, decode};

/// The developer store's sound kind.
fn native_pack() -> Option<std::sync::Arc<mvs_store::Pack>> {
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).ok()?;
    sound_pack(&store).ok()
}

fn is_setup(path: &str) -> bool {
    path.contains("/Setup/")
}

/// A medium entry of the store, the `k`-th short one (8 to 200 packets): its entry and its setup header.
pub(crate) fn oracle_entry(k: usize) -> Option<(Vec<u8>, Vec<u8>)> {
    let pack = native_pack()?;
    let mut n = 0;
    for path in pack.paths().filter(|p| !is_setup(p)).step_by(97) {
        let entry = pack.bytes(path).ok()?;
        let m = decode_medium(&entry, path).ok()?;
        if m.packets() < 8 || m.packets() > 200 {
            continue;
        }
        if n < k {
            n += 1;
            continue;
        }
        let setup = pack.bytes(&m.header.setup).ok()?;
        return Some((entry, setup));
    }
    None
}

/// The cache decodes a medium to the same samples as a decode of its own (the setup header parsed once and shared),
/// once: the second ask is the same samples, and a trim to 0 lets go only what nobody holds.
#[test]
fn the_cache_decodes_alike_and_once() {
    let Some(pack) = native_pack() else {
        eprintln!("no store: nothing checked");
        return;
    };
    let cache = super::cache::MediaCache::new();
    let paths: Vec<String> = pack.paths().filter(|p| !is_setup(p)).step_by(211).take(24).map(str::to_owned).collect();
    let mut held = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let id = u32::try_from(i).unwrap();
        let got = cache.medium(&pack, id, path).unwrap();
        assert!(got.decoded);
        let own = super::load(&pack, id, path).unwrap();
        assert_eq!((got.pcm.frames, got.pcm.channels), (own.frames, own.channels), "{path}");
        assert!(got.pcm.data.iter().zip(own.data.iter()).all(|(a, b)| a.to_bits() == b.to_bits()), "{path}");
        let again = cache.medium(&pack, id, path).unwrap();
        assert!(!again.decoded && Arc::ptr_eq(&again.pcm, &got.pcm), "{path}: decoded twice");
        if i % 2 == 0 {
            held.push(got.pcm);
        }
    }
    cache.trim();
    assert_eq!(cache.stats().entries, held.len(), "a trim to 0 keeps the held media alone");
}

/// The medium decoded packet by packet, planar, trimmed to its frames.
fn reference(m: &Medium, setup: &[u8], path: &str) -> Vec<Vec<f32>> {
    let id = read_header_ident(&m.header.ident).unwrap();
    let set = read_header_setup(setup, id.audio_channels, (id.blocksize_0, id.blocksize_1)).unwrap();
    let mut pwr = PreviousWindowRight::new();
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); usize::from(id.audio_channels)];
    let n = m.packets();
    assert_eq!(m.granule(0), 0, "{path}: the first packet's granule");
    for i in 0..n {
        let d: Vec<Vec<f32>> = read_audio_packet_generic(&id, &set, m.packet(i), &mut pwr).unwrap();
        for (o, c) in out.iter_mut().zip(d) {
            o.extend(c);
        }
    }
    // (per packet lewton's output can run ahead of the granules: a long block before a short one gives its
    // finished samples early, the short one fewer; the stream is the same)
    assert!(out[0].len() as u32 >= m.granule(n - 1), "{path}: the packets end before the last granule");
    let frames = m.header.frames as usize;
    for o in &mut out {
        assert!(o.len() >= frames, "{path}: {} frames decoded of {frames}", o.len());
        o.truncate(frames);
    }
    out
}

/// The oracle's Ogg of `path` through lewton's Ogg reader, planar.
fn ogg_reference(oracle: &mvs_store::Pack, path: &str) -> Option<Vec<Vec<f32>>> {
    let ogg = oracle.bytes(path).ok()?;
    let mut rd = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(ogg)).ok()?;
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); usize::from(rd.ident_hdr.audio_channels)];
    while let Some(p) = rd.read_dec_packet_generic::<Vec<Vec<f32>>>().unwrap() {
        for (o, c) in out.iter_mut().zip(p) {
            o.extend(c);
        }
    }
    Some(out)
}

/// Reads `n` frames of `s` (fewer at the end).
fn read(s: &mut Stream, ch: usize, n: usize) -> Vec<Vec<f32>> {
    let mut bufs = vec![vec![0.0f32; n]; ch];
    let mut outs: Vec<&mut [f32]> = bufs.iter_mut().map(|b| b.as_mut_slice()).collect();
    let got = s.read(&mut outs);
    for b in &mut bufs {
        b.truncate(got);
    }
    bufs
}

#[test]
fn decodes_and_seeks_as_lewton() {
    let Some(pack) = native_pack() else {
        eprintln!("decode test: no sound kind in the developer store, skipped");
        return;
    };
    let oracle = std::env::var_os("MVS_SOUND_ORACLE")
        .and_then(|d| mvs_store::Store::open(std::path::PathBuf::from(d)).ok())
        .and_then(|s| s.kind("sound").ok());
    let media: Vec<&str> = pack.paths().filter(|p| !is_setup(p)).collect();
    // about 24 media spread over the kind, and the first few with loop points and with more than two channels
    let mut picked: Vec<&str> = media.iter().step_by((media.len() / 24).max(1)).copied().collect();
    let (mut looped, mut wide) = (0, 0);
    for &p in &media {
        if looped >= 3 && wide >= 2 {
            break;
        }
        let e = pack.bytes(p).unwrap();
        let h = decode_medium(&e, p).unwrap().header;
        if h.loop_points.is_some() && looped < 3 {
            looped += 1;
            picked.push(p);
        } else if h.channels > 2 && wide < 2 {
            wide += 1;
            picked.push(p);
        }
    }
    let (mut checked, mut vs_ogg) = (0, 0);
    for path in &picked {
        let entry = pack.bytes(path).unwrap();
        let m = decode_medium(&entry, path).unwrap();
        let setup = pack.bytes(&m.header.setup).unwrap();
        let want = reference(&m, &setup, path);
        let frames = m.header.frames;
        let ch = want.len();
        if let Some(o) = &oracle
            && let Some(ogg) = ogg_reference(o, path)
        {
            for (c, w) in want.iter().enumerate() {
                assert!(ogg[c].len() >= w.len(), "{path}: the Ogg is shorter");
                assert_eq!(&ogg[c][..w.len()], &w[..], "{path} channel {c} against the Ogg");
            }
            vs_ogg += 1;
        }
        // whole
        let pcm = decode(1, &m, &setup).unwrap();
        assert_eq!(pcm.frames, frames, "{path}: frames");
        for (c, w) in want.iter().enumerate() {
            assert_eq!(pcm.channel(c), &w[..], "{path} channel {c}");
        }
        // streamed, from seeks
        let n = m.packets();
        let last = if n >= 2 { m.granule(n - 2) } else { 0 };
        let mut at = vec![frames / 3, last + (frames - last) / 2];
        if let Some([a, _]) = m.header.loop_points {
            at.push(a.min(frames));
        }
        let coded = Arc::new(Coded::new(1, entry.clone().into_boxed_slice(), &setup).unwrap());
        let mut s = Stream::open(coded, at[0]).unwrap();
        for (k, &from) in at.iter().enumerate() {
            if k > 0 {
                s.seek(from);
            }
            let got = read(&mut s, ch, 3000);
            let n = 3000.min((frames - from) as usize);
            for c in 0..ch {
                assert_eq!(got[c], want[c][from as usize..from as usize + n], "{path} stream from {from}");
            }
        }
        // back to the start, read to the end
        s.seek(0);
        let got = read(&mut s, ch, frames as usize + 100);
        for c in 0..ch {
            assert_eq!(got[c], want[c], "{path} stream whole");
        }
        checked += 1;
    }
    eprintln!("decode test: {checked} media ({looped} looped, {wide} wide), {vs_ogg} against the oracle's Ogg");
    assert!(checked >= 20);
}
