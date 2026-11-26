//! The updater against a feed served on 127.0.0.1 by the test itself: the check, the download with its size and
//! hash refusals, the unpack, the launcher's updater's events, and the copy over the exe's folder (the start's clear
//! is in update_clear.rs, a process of its own: the stage's lock is the process's).
#![cfg(windows)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flate2::Crc;
use flate2::write::DeflateEncoder;
use mvs_app::update::{self, Place, UpdateError, UpdateEvent, Updater};
use mvs_release::Release;
use sha2::{Digest, Sha256};

/// The feed's files by URL path.
type Files = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// Serves `files` on 127.0.0.1 (one request a connection); the feed's address.
fn serve(files: Files) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut first = String::new();
            if r.read_line(&mut first).is_err() {
                continue;
            }
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
            }
            let path = first.split(' ').nth(1).unwrap_or("").to_owned();
            let body = files.lock().unwrap().get(&path).cloned();
            let (status, body) = match body {
                Some(b) => ("200 OK", b),
                None => ("404 Not Found", b"not here".to_vec()),
            };
            let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/feed/")
}

/// A zip of `files` (name, bytes): the first stored, the rest deflated.
fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (k, (name, data)) in files.iter().enumerate() {
        let mut crc = Crc::new();
        crc.update(data);
        let (method, packed) = if k == 0 {
            (0u16, data.to_vec())
        } else {
            let mut e = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            (8u16, e.finish().unwrap())
        };
        let at = out.len() as u32;
        let fields = |sig: u32, v: &mut Vec<u8>| {
            v.extend_from_slice(&sig.to_le_bytes());
            if sig == 0x0201_4b50 {
                v.extend_from_slice(&20u16.to_le_bytes());
            }
            v.extend_from_slice(&20u16.to_le_bytes());
            v.extend_from_slice(&0x0800u16.to_le_bytes());
            v.extend_from_slice(&method.to_le_bytes());
            v.extend_from_slice(&[0; 4]);
            v.extend_from_slice(&crc.sum().to_le_bytes());
            v.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(name.len() as u16).to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes());
        };
        fields(0x0403_4b50, &mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);
        fields(0x0201_4b50, &mut central);
        central.extend_from_slice(&[0; 6]);
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_at = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Release `version`, its zip `file` served at `base`.
fn release(base: &str, version: &str, file: &str, bytes: &[u8]) -> Release {
    let sha256 = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    Release {
        version: version.into(),
        file: file.into(),
        url: format!("{base}{file}"),
        size: bytes.len() as u64,
        sha256,
    }
}

fn put(files: &Files, path: &str, bytes: Vec<u8>) {
    files.lock().unwrap().insert(path.to_owned(), bytes);
}

/// A scratch folder of this test's, started empty.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mvs-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An installed 0.1.0: its exe, its licence, a file the release drops, and the player's data.
fn installed(root: &Path) -> Place {
    let app = root.join("MVS Classic");
    std::fs::create_dir_all(app.join("data")).unwrap();
    std::fs::write(app.join("mvs.exe"), b"exe 0.1.0").unwrap();
    std::fs::write(app.join("LICENSE.txt"), b"licence 0.1.0").unwrap();
    std::fs::write(app.join("data/settings.json"), b"{\"keep\": true}").unwrap();
    Place { app_dir: app, exe: "mvs.exe".into() }
}

const FILES: [(&str, &[u8]); 3] =
    [("mvs.exe", b"exe 0.1.1"), ("LICENSE.txt", b"licence 0.1.1"), ("licenses/LICENSE-CUE4Parse.txt", b"cue4parse")];

#[test]
fn check_download_unpack_and_copy_over() {
    let root = scratch("flow");
    let place = installed(&root);
    let files: Files = Arc::default();
    let feed = serve(files.clone());
    let z = zip(&FILES);
    let r = release(&feed, "0.1.1", "MVS-Classic-0.1.1.zip", &z);
    put(&files, "/feed/MVS-Classic-0.1.1.zip", z.clone());
    let api = format!("{feed}latest");

    // No feed there yet: the status is the error.
    assert_eq!(update::latest(&api, "0.1.0"), Err(UpdateError::FeedStatus(404)));
    put(&files, "/feed/latest", r.to_github().into_bytes());
    assert_eq!(update::latest(&api, "0.1.0"), Ok(Some(r.clone())));
    assert_eq!(update::latest(&api, "0.1.1"), Ok(None));
    assert_eq!(update::latest(&api, "0.2.0"), Ok(None));

    // A hash or a size that differs is refused.
    let wrong = Release { sha256: "0".repeat(64), ..r.clone() };
    assert_eq!(update::stage(&place, &wrong, |_| {}), Err(UpdateError::Hash));
    let long = Release { size: r.size + 10, ..r.clone() };
    assert_eq!(update::stage(&place, &long, |_| {}), Err(UpdateError::Short { got: r.size, size: r.size + 10 }));
    let short = Release { size: r.size - 10, ..r.clone() };
    assert_eq!(update::stage(&place, &short, |_| {}), Err(UpdateError::Long(r.size - 10)));

    // Downloaded, checked, unpacked.
    let mut seen = Vec::new();
    let dir = update::stage(&place, &r, |pc| seen.push(pc)).unwrap();
    assert_eq!(seen.last(), Some(&100));
    assert_eq!(dir, place.stage().join("0.1.1"));
    for (name, bytes) in FILES {
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes, "{name}");
    }
    assert!(!place.stage().join(&r.file).exists(), "the zip is removed once unpacked");

    // Only a release under the folder's data\update\ copies itself over it.
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    assert!(matches!(update::finish::finish(&elsewhere, &place.app_dir, None), Err(UpdateError::NotUnder { .. })));
    // The copy over: the release's files replace the folder's, data\ untouched.
    update::finish::finish(&dir, &place.app_dir, Some(u32::MAX)).unwrap();
    let app = &place.app_dir;
    assert_eq!(std::fs::read(app.join("mvs.exe")).unwrap(), b"exe 0.1.1");
    assert_eq!(std::fs::read(app.join("LICENSE.txt")).unwrap(), b"licence 0.1.1");
    assert_eq!(std::fs::read(app.join("licenses/LICENSE-CUE4Parse.txt")).unwrap(), b"cue4parse");
    assert_eq!(std::fs::read(app.join("data/settings.json")).unwrap(), b"{\"keep\": true}");
    assert!(app.join("data/update/0.1.1/mvs.exe").is_file(), "the stage stays for the next start to clear");
    assert_eq!(std::fs::read(app.join("data/update/previous/mvs.exe")).unwrap(), b"exe 0.1.0");
    let _ = std::fs::remove_dir_all(&root);
}

/// Writes `files` (name, bytes) under `dir`, their folders made.
fn write_all(dir: &Path, files: &[(&str, &[u8])]) {
    for (name, bytes) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

#[test]
fn only_the_releases_listed_files_are_replaced_or_removed() {
    let root = scratch("listed");
    let app = root.join("MVS Classic");
    // a listed 0.2.0, with files of the player's beside and inside its folders
    write_all(
        &app,
        &[
            ("MVSC Launcher.exe", b"launcher 0.2.0"),
            ("game/MVS Classic.exe", b"exe 0.2.0"),
            ("game/old/dropped.dll", b"dropped"),
            (
                "game/release-files.txt",
                b"MVSC Launcher.exe\ngame/MVS Classic.exe\ngame/old/dropped.dll\ngame/release-files.txt\n",
            ),
            ("game/my-notes.txt", b"mine"),
            ("resources/mine.txt", b"mine"),
            ("notes.txt", b"mine"),
            ("data/settings.json", b"{\"keep\": true}"),
        ],
    );
    let place = Place { app_dir: app.clone(), exe: "MVSC Launcher.exe".into() };
    let mine = ["game/my-notes.txt", "resources/mine.txt", "notes.txt", "data/settings.json"];
    for version in ["0.2.1", "0.2.2"] {
        let dir = place.stage().join(version);
        let exe = format!("exe {version}");
        write_all(
            &dir,
            &[
                ("MVSC Launcher.exe", b"launcher"),
                ("game/MVS Classic.exe", exe.as_bytes()),
                ("game/release-files.txt", b"MVSC Launcher.exe\ngame/MVS Classic.exe\ngame/release-files.txt\n"),
            ],
        );
        update::finish::finish(&dir, &app, None).unwrap();
        assert_eq!(std::fs::read(app.join("game/MVS Classic.exe")).unwrap(), exe.as_bytes());
        for f in mine {
            assert!(app.join(f).is_file(), "{f}, the player's, is gone after {version}");
            assert!(!app.join("data/update/previous").join(f).exists(), "{f} was set aside");
        }
    }
    // the file the old release listed and the new one dropped: set aside at the first update, its folder gone
    assert!(!app.join("game/old").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_list_naming_data_is_not_trusted() {
    let root = scratch("baddata");
    let app = root.join("MVS Classic");
    write_all(
        &app,
        &[
            ("game/MVS Classic.exe", b"exe 0.2.0"),
            ("game/release-files.txt", b"game/MVS Classic.exe\ndata/settings.json\n"),
            ("data/settings.json", b"{\"keep\": true}"),
        ],
    );
    let place = Place { app_dir: app.clone(), exe: "MVSC Launcher.exe".into() };
    let dir = place.stage().join("0.2.1");
    write_all(&dir, &[("game/MVS Classic.exe", b"exe 0.2.1")]);
    update::finish::finish(&dir, &app, None).unwrap();
    assert_eq!(std::fs::read(app.join("game/MVS Classic.exe")).unwrap(), b"exe 0.2.1");
    assert_eq!(std::fs::read(app.join("data/settings.json")).unwrap(), b"{\"keep\": true}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_old_layouts_top_level_files_are_set_aside() {
    let root = scratch("layout");
    let app = root.join("MVS Classic");
    std::fs::create_dir_all(app.join("data")).unwrap();
    std::fs::create_dir_all(app.join("licenses")).unwrap();
    for (name, bytes) in [
        ("MVSC Launcher.exe", &b"launcher 0.1.0"[..]),
        ("MVS Classic.exe", b"exe 0.1.0"),
        ("online.json", b"{}"),
        ("licenses/NOTICE-importer.txt", b"n"),
        ("data/settings.json", b"{\"keep\": true}"),
    ] {
        std::fs::write(app.join(name), bytes).unwrap();
    }
    let place = Place { app_dir: app.clone(), exe: "MVSC Launcher.exe".into() };
    let dir = place.stage().join("0.1.1");
    std::fs::create_dir_all(dir.join("game/licenses")).unwrap();
    for (name, bytes) in [
        ("MVSC Launcher.exe", &b"launcher 0.1.1"[..]),
        ("LICENSE.txt", b"licence"),
        ("game/MVS Classic.exe", b"exe 0.1.1"),
        ("game/online.json", b"{}"),
        ("game/licenses/NOTICE-importer.txt", b"n"),
    ] {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    update::finish::finish(&dir, &app, None).unwrap();
    assert_eq!(std::fs::read(app.join("MVSC Launcher.exe")).unwrap(), b"launcher 0.1.1");
    assert_eq!(std::fs::read(app.join("game/MVS Classic.exe")).unwrap(), b"exe 0.1.1");
    for old in ["MVS Classic.exe", "online.json", "licenses"] {
        assert!(!app.join(old).exists(), "{old} is left at the top");
        assert!(app.join("data/update/previous").join(old).exists(), "{old} is not set aside");
    }
    assert_eq!(std::fs::read(app.join("data/settings.json")).unwrap(), b"{\"keep\": true}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_copy_puts_the_old_release_back() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = scratch("back");
    let place = installed(&root);
    let dir = place.stage().join("0.1.1");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in [("LICENSE.txt", &b"licence 0.1.1"[..]), ("mvs.exe", b"exe 0.1.1"), ("zz.txt", b"new")] {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    // (a file of the release no one can read: its copy fails for good, after the two names before it were done)
    let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(dir.join("zz.txt")).unwrap();
    assert!(matches!(update::finish::finish(&dir, &place.app_dir, None), Err(UpdateError::Io(_))));
    drop(held);
    let app = &place.app_dir;
    assert_eq!(std::fs::read(app.join("mvs.exe")).unwrap(), b"exe 0.1.0");
    assert_eq!(std::fs::read(app.join("LICENSE.txt")).unwrap(), b"licence 0.1.0");
    assert!(!app.join("zz.txt").exists());
    assert_eq!(std::fs::read(app.join("data/settings.json")).unwrap(), b"{\"keep\": true}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_zip_of_the_release_folder_and_a_bad_zip() {
    let root = scratch("folder");
    let place = installed(&root);
    let files: Files = Arc::default();
    let feed = serve(files.clone());
    // (the zip holds the release's folder rather than its files)
    let z =
        zip(&[("MVS-Classic-0.1.1/mvs.exe", &b"exe 0.1.1"[..]), ("MVS-Classic-0.1.1/LICENSE.txt", &b"licence"[..])]);
    let r = release(&feed, "0.1.1", "a.zip", &z);
    put(&files, "/feed/a.zip", z);
    let dir = update::stage(&place, &r, |_| {}).unwrap();
    assert_eq!(dir, place.stage().join("0.1.1").join("MVS-Classic-0.1.1"));
    // A zip with no exe, and one whose names climb out.
    let z = zip(&[("LICENSE.txt", &b"licence"[..])]);
    let r = release(&feed, "0.1.1", "b.zip", &z);
    put(&files, "/feed/b.zip", z);
    assert_eq!(update::stage(&place, &r, |_| {}), Err(UpdateError::NoExe("mvs.exe".into())));
    let z = zip(&[("mvs.exe", &b"x"[..]), ("../evil.txt", &b"x"[..])]);
    let r = release(&feed, "0.1.1", "c.zip", &z);
    put(&files, "/feed/c.zip", z);
    assert!(matches!(update::stage(&place, &r, |_| {}), Err(UpdateError::Unpack(_))));
    assert!(!place.stage().join("evil.txt").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// The launcher's updater: its events until `want` says it has them.
fn events_until(u: &mut Updater, want: impl Fn(&[UpdateEvent]) -> bool) -> Vec<UpdateEvent> {
    let t0 = Instant::now();
    let mut all = Vec::new();
    while !want(&all) {
        assert!(t0.elapsed() < Duration::from_secs(30), "no answer: {all:?}");
        all.extend(u.poll());
        std::thread::sleep(Duration::from_millis(10));
    }
    all
}

#[test]
fn the_launchers_updater() {
    let root = scratch("updater");
    let place = installed(&root);
    let files: Files = Arc::default();
    let feed = serve(files.clone());
    let z = zip(&FILES);
    let r = release(&feed, "0.1.1", "MVS-Classic-0.1.1.zip", &z);
    put(&files, "/feed/MVS-Classic-0.1.1.zip", z);
    put(&files, "/feed/latest", r.to_github().into_bytes());
    let api = format!("{feed}latest");

    // No feed: never offered.
    let mut none = Updater::new(None, Some(place.clone()));
    assert!(!none.enabled());
    none.check("0.1.0");
    assert_eq!(events_until(&mut none, |e| !e.is_empty()), [UpdateEvent::None]);

    let mut u = Updater::new(Some(&api), Some(place.clone()));
    // (a download before a check found a release)
    u.download();
    assert!(matches!(events_until(&mut u, |e| !e.is_empty()).as_slice(), [UpdateEvent::DownloadError(_)]));
    u.check("0.1.1");
    assert_eq!(events_until(&mut u, |e| !e.is_empty()), [UpdateEvent::None]);
    u.check("0.1.0");
    assert_eq!(events_until(&mut u, |e| !e.is_empty()), [UpdateEvent::Available("0.1.1".into())]);
    assert_eq!(u.available(), Some("0.1.1"));
    u.download();
    assert!(u.busy());
    let ev =
        events_until(&mut u, |e| e.iter().any(|e| matches!(e, UpdateEvent::Ready(_) | UpdateEvent::DownloadError(_))));
    assert_eq!(ev.last(), Some(&UpdateEvent::Ready("0.1.1".into())));
    assert!(ev.contains(&UpdateEvent::Progress(100)));
    assert!(place.stage().join("0.1.1/mvs.exe").is_file());

    // A feed gone: the check's error is said.
    files.lock().unwrap().clear();
    u.check("0.1.0");
    let ev = events_until(&mut u, |e| !e.is_empty());
    assert_eq!(ev, [UpdateEvent::CheckError(UpdateError::FeedStatus(404).to_string())]);
    let _ = std::fs::remove_dir_all(&root);
}
