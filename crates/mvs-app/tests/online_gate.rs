//! The online gate's checks: the update feed served on 127.0.0.1 by the test (a newer release, the same, a feed that
//! never answers, one that is not there) and a store of an older import, folded into the hub's gate.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use mvs_app::online::gate::{self, FEED_WAIT, Feed, FeedCheck, Gate};

/// A feed (the GitHub API's latest release) that offers `version`; its address.
fn feed_of(version: &str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let body = mvs_release::Release {
        version: version.to_owned(),
        file: mvs_release::game_zip_name(version),
        url: "http://127.0.0.1/z.zip".to_owned(),
        size: 1,
        sha256: "0".repeat(64),
    }
    .to_github();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
            }
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(body.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}/feed/latest")
}

/// The answer once it is not Asking, waited for at most `most`.
fn answer_within(f: &mut FeedCheck, most: Duration) -> Feed {
    let until = Instant::now() + most;
    loop {
        let a = f.answer();
        if a != Feed::Asking || Instant::now() > until {
            return a;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_old_release_sees_the_newer_one_on_the_feed() {
    let feed = feed_of("9.0.0");
    let mut f = FeedCheck::start(Some(&feed), "0.1.0", FEED_WAIT);
    assert_eq!(answer_within(&mut f, Duration::from_secs(10)), Feed::Newer("9.0.0".to_owned()));
    let mut same = FeedCheck::start(Some(&feed), "9.0.0", FEED_WAIT);
    assert_eq!(answer_within(&mut same, Duration::from_secs(10)), Feed::Done);
}

#[test]
fn a_feed_that_never_answers_opens_online_after_the_wait() {
    // (a listener that takes the connection and says nothing)
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let feed = format!("http://127.0.0.1:{}/feed/", l.local_addr().unwrap().port());
    let held = std::thread::spawn(move || {
        let s = l.accept();
        std::thread::sleep(Duration::from_secs(3));
        drop(s);
    });
    let wait = Duration::from_millis(500);
    let started = Instant::now();
    let mut f = FeedCheck::start(Some(&feed), "0.1.0", wait);
    assert_eq!(f.answer(), Feed::Asking);
    assert_eq!(answer_within(&mut f, Duration::from_secs(2)), Feed::Done);
    assert!(started.elapsed() >= wait, "not before the wait");
    assert!(started.elapsed() < Duration::from_secs(2), "at the wait");
    let _ = held.join();
}

#[test]
fn no_feed_there_is_no_gate() {
    // (a port nothing listens on: the connection refused at once)
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut f = FeedCheck::start(Some(&format!("http://127.0.0.1:{port}/feed/")), "0.1.0", FEED_WAIT);
    let started = Instant::now();
    assert_eq!(answer_within(&mut f, FEED_WAIT * 2), Feed::Done);
    assert!(started.elapsed() < FEED_WAIT, "a refused feed does not wait it out");
}

#[test]
fn a_stale_store_gates_online_until_imported_and_a_newer_release_outranks_it() {
    let data = std::env::temp_dir().join(format!("mvs-gate-test-{}", std::process::id()));
    let hub = mvs_app::online::glue::start_offline(&data);
    let mut h = hub.borrow_mut();
    let stale: mvs_store::Manifest =
        serde_json::from_value(serde_json::json!({ "format": 1, "buildId": "b", "kinds": {} })).unwrap();
    // (no feed: answered at once; the store not open yet: Checking)
    gate::arm(&mut h, None, "0.1.0");
    assert_eq!(h.gate(), Some(&Gate::Checking));
    gate::poll(&mut h, None);
    assert_eq!(h.gate(), Some(&Gate::Checking));
    gate::poll(&mut h, Some(&stale));
    assert_eq!(h.gate(), Some(&Gate::Import));
    // (a newer release on the feed first)
    let feed = feed_of("9.0.0");
    gate::arm(&mut h, Some(&feed), "0.1.0");
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        gate::poll(&mut h, Some(&stale));
        if h.gate() != Some(&Gate::Checking) && h.gate() != Some(&Gate::Import) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(h.gate(), Some(&Gate::Update { version: "9.0.0".to_owned() }));
}
