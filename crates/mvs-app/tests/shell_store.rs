//! A store that does not open leaves the shell as it was: its GPU kept (the launcher draws on), no store.

use mvs_app::app::{Shell, ShellOptions};

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mvs-shell-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_failed_open_keeps_the_gpu() {
    let data = scratch("data");
    let o = ShellOptions {
        store: None,
        data: data.clone(),
        size: [320, 180],
        full_screen: false,
        adapter: None,
        ui: true,
        audio: false,
        audio_offline: false,
        online_defaults: data.join("online.json"),
    };
    let mut shell = Shell::new(&o).unwrap();
    // no store folder at all
    assert!(shell.open_store(&data.join("store")).is_err());
    assert!(shell.store().is_none());
    let _ = &shell.gpu().adapter;
    // a store with no kinds: the renderer's assets do not open
    let store = data.join("empty");
    std::fs::create_dir_all(&store).unwrap();
    let m = mvs_store::Manifest {
        format: mvs_store::STORE_FORMAT,
        build_id: "test".into(),
        catalog: None,
        catalog_version: None,
        kinds: Default::default(),
    };
    mvs_store::write_manifest(&store, &m).unwrap();
    assert!(shell.open_store(&store).is_err());
    assert!(shell.store().is_none());
    let _ = &shell.gpu().adapter;
    // and the next open is not "a store is open already"
    let e = shell.open_store(&store).unwrap_err().to_string();
    assert!(!e.contains("open already"), "{e}");
    drop(shell);
    let _ = std::fs::remove_dir_all(&data);
}
