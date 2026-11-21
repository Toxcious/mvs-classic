//! A backend stopped while the service's and the sessions' asks are out on it (the oracle keeps them per Eos object,
//! which goes with it): a pending browse and online list answer at once, the next browse searches again, and the asks
//! of the backend signed in next (counted from 1 again) reach their own owners, not the old asks' ones.

use mvs_net::backend::BackendKind;
use mvs_net::backend::memory::{MemoryNet, MemoryNetOptions};
use mvs_net::service::api::{BrowseWhy, ClosedWhy, NetEvent, NetRequest, OffWhy, SessionTarget};
use mvs_net::service::{NetService, PageId, ServiceConfig, ServiceOptions};
use mvs_protocol::version::PROTOCOL;

const RELAY: &str = "127.0.0.1:7777";

fn config() -> ServiceConfig {
    ServiceConfig { relay: Some(RELAY.to_string()), ..ServiceConfig::default() }
}

/// Runs the service and the net to `to`, 1 ms at a time, keeping every page's events.
fn run(
    svc: &mut NetService,
    net: &MemoryNet,
    pages: &[PageId],
    seen: &mut Vec<(PageId, NetEvent)>,
    from: f64,
    to: f64,
) {
    let mut t = from;
    while t <= to {
        net.pump(t);
        svc.pump(t);
        take(svc, pages, seen);
        t += 1.0;
    }
}

fn take(svc: &mut NetService, pages: &[PageId], seen: &mut Vec<(PageId, NetEvent)>) {
    for p in pages {
        for e in svc.events(*p) {
            seen.push((*p, e));
        }
    }
}

fn browsed(seen: &[(PageId, NetEvent)], page: PageId, id: u32) -> Option<BrowseWhy> {
    seen.iter().find_map(|(p, e)| match e {
        NetEvent::Browsed { id: i, why, .. } if *p == page && *i == id => Some(why.clone()),
        _ => None,
    })
}

#[test]
fn stopped_mid_browse_and_mid_create() {
    // (the relay's cores answer with their first message: the page's Opened waits for it)
    let o = MemoryNetOptions { search_lag_ms: 50.0, relay_ids: true, core_proto: Some(PROTOCOL), ..Default::default() };
    let net = MemoryNet::new(o, 0.0);
    let mut svc = NetService::new(ServiceOptions::default(), 0.0);
    svc.configure(config(), 0.0);
    let browser = svc.open_page(0.0);
    let host = svc.open_page(0.0);
    let pages = [browser, host];
    let mut seen = Vec::new();

    // (the first relay: a browse's search, an online list's ask and a lobby's create out, none answered)
    svc.start_backend(BackendKind::Relay, Box::new(net.player(Some("A"), BackendKind::Relay)), 1.0);
    svc.take(browser, NetRequest::Browse { id: 1 }, 2.0);
    svc.take(browser, NetRequest::Who { id: 7 }, 2.0);
    svc.take(host, NetRequest::Open(SessionTarget::Host), 2.0);
    take(&mut svc, &pages, &mut seen);
    assert_eq!(browsed(&seen, browser, 1), None, "the search is out");

    svc.stop_backend(BackendKind::Relay, OffWhy::Off, 3.0);
    take(&mut svc, &pages, &mut seen);
    assert!(
        matches!(browsed(&seen, browser, 1), Some(BrowseWhy::Offline { .. })),
        "the pending browse fails at the stop: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|(p, e)| *p == browser && matches!(e, NetEvent::Who { id: 7, players, .. } if players.is_empty())),
        "the pending online list fails at the stop: {seen:?}"
    );
    assert!(
        seen.iter().any(|(p, e)| *p == host
            && matches!(e, NetEvent::Closed(ClosedWhy::BackendOff { kind: BackendKind::Relay, .. }))),
        "the hosting session ends: {seen:?}"
    );

    // (signed in again: its asks count from 1, the lobby's create first, then the browse's search)
    svc.start_backend(BackendKind::Relay, Box::new(net.player(Some("B"), BackendKind::Relay)), 10.0);
    seen.clear();
    svc.take(host, NetRequest::Open(SessionTarget::Host), 11.0);
    svc.take(browser, NetRequest::Browse { id: 2 }, 11.0);
    run(&mut svc, &net, &pages, &mut seen, 11.0, 200.0);
    assert!(
        seen.iter().any(|(p, e)| *p == host && matches!(e, NetEvent::Opened { code, .. } if !code.is_empty())),
        "the new create's answer reaches the new host session: {seen:?}"
    );
    assert!(!seen.iter().any(|(p, e)| *p == host && matches!(e, NetEvent::Closed(_))), "the host stays: {seen:?}");
    assert_eq!(browsed(&seen, browser, 2), Some(BrowseWhy::All), "the next browse searches again: {seen:?}");
}
