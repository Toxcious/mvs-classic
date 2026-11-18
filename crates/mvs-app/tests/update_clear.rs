//! The start's clear of the last update's folder, in a process of its own: a download elsewhere in the process
//! (update.rs's tests) holds the stage's lock and keeps it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mvs_app::update::{Place, Updater};

#[test]
fn a_new_start_clears_the_last_update() {
    let root: PathBuf = std::env::temp_dir().join(format!("mvs-update-clear-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let place = Place { app_dir: root.join("MVS Classic"), exe: "mvs.exe".into() };
    std::fs::create_dir_all(place.app_dir.join("data")).unwrap();
    std::fs::write(place.app_dir.join("data/settings.json"), b"{}").unwrap();
    std::fs::create_dir_all(place.stage().join("0.1.1")).unwrap();
    std::fs::write(place.stage().join("0.1.1/mvs.exe"), b"x").unwrap();
    let _u = Updater::new(None, Some(place.clone()));
    let t0 = Instant::now();
    while place.stage().exists() {
        assert!(t0.elapsed() < Duration::from_secs(20), "data\\update\\ was not cleared");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(place.app_dir.join("data/settings.json").is_file());
    let _ = std::fs::remove_dir_all(&root);
}
