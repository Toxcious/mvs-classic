//! The pack writer: a pack written and read back; and, against the developer's store, the writer's zstd frames are
//! the oracle writer's (each zstd entry of the object pack's first entries re-encoded to its stored bytes).

use std::fs::File;
use std::path::PathBuf;

use mvs_store::writer::{PackWriter, encode_entry};
use mvs_store::{Pack, Store, dev_store_dir};

fn tmp_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn writes_a_pack_and_reads_it_back() {
    let dir = tmp_dir("pack-writer");
    let file = dir.join("object@11-test.pack");
    let big: Vec<u8> = (0..200_000u32).flat_map(|i| (i % 97).to_le_bytes()).collect();
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let noise: Vec<u8> = (0..10_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 32) as u8
        })
        .collect();
    let entries: Vec<(&str, &str, Vec<u8>)> = vec![
        ("/Game/B.B", "application/json", b"{\"a\":1}".to_vec()),
        ("/Game/A.A", "application/octet-stream", big.clone()),
        ("/Game/\u{10000}", "application/json", b"{}".to_vec()),
        ("/Game/\u{FFFF}", "application/json", b"{}".to_vec()),
        ("/Game/Empty", "application/json", Vec::new()),
        ("/Game/Noise", "application/octet-stream", noise.clone()),
        ("/Game/Ogg", "audio/ogg", big.clone()),
    ];
    let mut w = PackWriter::create(&file).unwrap();
    for (p, t, b) in &entries {
        w.add(p, t, b).unwrap();
    }
    w.add("/Game/A.A", "application/json", b"x").unwrap(); // a path added twice keeps the first
    let sum = w.finish().unwrap();
    assert_eq!(sum.entries, entries.len() as u64);
    assert_eq!(sum.bytes, std::fs::metadata(&file).unwrap().len());
    assert!(!dir.join("object@11-test.pack.tmp").exists());

    let pack = Pack::open("object", &file).unwrap();
    let paths: Vec<&str> = pack.paths().collect();
    // JS's order: by UTF-16 code units, so U+10000 (D800 DC00) before U+FFFF
    assert_eq!(
        paths,
        ["/Game/A.A", "/Game/B.B", "/Game/Empty", "/Game/Noise", "/Game/Ogg", "/Game/\u{10000}", "/Game/\u{FFFF}"]
    );
    for (p, t, b) in &entries {
        assert_eq!(&pack.bytes(p).unwrap(), b, "{p}");
        assert_eq!(pack.content_type(p), Some(*t), "{p}");
    }
    assert_eq!(pack.record("/Game/A.A").unwrap().codec, 1, "a large compressible entry is zstd");
    assert_eq!(pack.record("/Game/Ogg").unwrap().codec, 0, "an ogg is stored");
    assert_eq!(pack.record("/Game/Noise").unwrap().codec, 0, "noise saves too little");
    drop(pack);

    // a writer dropped unfinished leaves no file
    let mut w = PackWriter::create(&dir.join("x.pack")).unwrap();
    w.add("/a", "application/json", b"{}").unwrap();
    drop(w);
    assert!(!dir.join("x.pack.tmp").exists() && !dir.join("x.pack").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The first zstd entries of the developer's object pack, decoded and encoded again, give the stored bytes the oracle's
/// writer wrote (the same zstd, level and frame).
#[test]
fn zstd_frames_are_the_ts_writers() {
    let store = match Store::open(dev_store_dir()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no store ({e}): skipped");
            return;
        }
    };
    let Ok(pack) = store.kind("object") else {
        eprintln!("no object kind in the store: skipped");
        return;
    };
    let file = File::open(store.dir().join(&store.manifest().kinds["object"].file)).unwrap();
    let mut checked = 0;
    for p in pack.paths() {
        let rec = *pack.record(p).unwrap();
        if rec.codec != 1 {
            continue;
        }
        let mut stored = vec![0u8; rec.length as usize];
        read_at(&file, &mut stored, rec.offset);
        let bytes = pack.bytes(p).unwrap();
        let (enc, codec) = encode_entry(&bytes, pack.content_type(p).unwrap()).unwrap();
        assert_eq!(codec, 1, "{p}");
        assert!(enc[..] == stored[..], "{p}: the zstd frame differs from the oracle writer's");
        checked += 1;
        if checked == 500 {
            break;
        }
    }
    eprintln!("{checked} zstd entries re-encoded to the oracle writer's bytes");
}

fn read_at(f: &File, buf: &mut [u8], offset: u64) {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            done += f.seek_read(&mut buf[done..], offset + done as u64).unwrap();
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, offset).unwrap();
    }
}
