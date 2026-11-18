//! The launcher's Test connection end to end: the Online tab starts the game's exe as `mvs --net-test` on a data
//! folder and an online.json of the test's own, reads its JSON back and lists it. The relay is down (nothing listens
//! at its address), Epic is on with no EOS files (no MVS_EOS_* variables, none beside the test's exe): it says why
//! it is off, never signs in. The relay with UDP closed is the result's vectors (launcher::net_test's unit tests).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mvs_app::launcher::game::GameArgs;
use mvs_app::launcher::net_test::{LanPort, TestResult, TestUses};
use mvs_app::launcher::online::OnlineTab;
use mvs_net::service::api::OffWhy;
use mvs_settings::online::OnlineSetting;
use mvs_ui::launcher::{Tone, words as w};

/// A folder of the test's own under the temp folder, empty.
fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mvs-net-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("made");
    d
}

/// A port nothing listens on now (bound and let go).
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bound");
    l.local_addr().expect("address").port()
}

#[test]
fn a_down_relay_and_epic_off_are_said() {
    // (the child finds no EOS files: the variables are the test's own process's, which the child inherits)
    assert!(
        std::env::var_os("MVS_EOS_DLL").is_none() && std::env::var_os("MVS_EOS_CREDENTIALS").is_none(),
        "run the test without MVS_EOS_DLL / MVS_EOS_CREDENTIALS"
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_mvs"));
    assert!(!exe.with_file_name(mvs_release::EOS_DLL).exists(), "EOS beside the test's exe");
    let dir = folder("down");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).expect("made");
    let lan = free_port();
    let defaults = dir.join("online.json");
    let config = format!(r#"{{"relay":"127.0.0.1:1","eos":true,"lan":{lan},"direct":false}}"#);
    std::fs::write(&defaults, config).expect("written");
    // the child alone: its JSON
    let out = std::process::Command::new(&exe)
        .args(["--net-test", "--direct-loopback", "--data"])
        .arg(&data)
        .arg("--online-defaults")
        .arg(&defaults)
        .output()
        .expect("ran");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let line = String::from_utf8_lossy(&out.stdout);
    let r: TestResult = serde_json::from_str(line.trim()).expect("its JSON");
    assert!(r.relay.as_ref().is_err_and(|why| !why.is_empty()), "{r:?}");
    assert_eq!(r.udp, None);
    assert!(matches!(r.epic, Err(OffWhy::Missing { .. })), "{r:?}");
    assert_eq!(r.lan, LanPort::Free(lan));
    assert_eq!(r.uses, TestUses::Lan);
    // the launcher's tab: started, polled, listed, copied
    let mut tab = OnlineTab::new(&defaults);
    let game = GameArgs { exe, data: Some(data), online_defaults: defaults, adapter: None, pass_on: Vec::new() };
    let s = OnlineSetting::default();
    tab.start_test(&game);
    assert!(tab.view(&s).testing && tab.copy_test().is_none());
    let until = Instant::now() + Duration::from_secs(30);
    while !tab.poll_test() {
        assert!(Instant::now() < until, "no result in time");
        std::thread::sleep(Duration::from_millis(20));
    }
    let v = tab.view(&s);
    assert!(!v.testing);
    assert_eq!(v.test.len(), 4, "{:?}", v.test);
    assert_eq!(v.test[0].tone, Tone::Bad);
    assert_eq!(v.test[3].text, w::TEST_USES_LAN);
    // the line from the test: LAN alone is online
    assert_eq!(v.line.tone, Tone::Ok, "{:?}", v.line);
    let text = tab.copy_test().expect("a result");
    assert!(text.starts_with("mvs ") && text.lines().count() == 5, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
