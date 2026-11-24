//! The LAN announcement's packet, listener and sockets,
//! on 127.0.0.1 and 127.255.255.255 only. The service-dependent parts of that file (a browse over the net service, the
//! LAN setting's port check) belong to the service and the settings.

use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mvs_net::lan::announce::{
    ANNOUNCE_EVERY_MS, ANNOUNCE_MAX_BYTES, Announcement, AnnouncerOptions, Dropped, HEARD_FOR_MS, HEARD_MAX,
    LanAnnouncer, LanListener, ListenerOptions, LogFn, SENDER_LOBBIES, SENDER_PACKETS, broadcast_targets,
    decode_announcement, encode_announcement,
};
use mvs_net::lan::direct_code::{DirectCode, DirectWhy, Local, Nic, decode_direct_code, encode_direct_code};
use mvs_protocol::lobby_listing::{LISTING_V, ListingState, LobbyListing, listing_text};
use mvs_protocol::lobby_rules::LobbyStyle;

const FP: &str = "0123456789abcdef";

fn info() -> String {
    listing_text(&LobbyListing {
        v: LISTING_V,
        name: "Velma".to_owned(),
        icon: String::new(),
        compat: "compat1".to_owned(),
        app: "test".to_owned(),
        style: LobbyStyle::Versus,
        maps: vec!["M001".to_owned()],
        map_count: 1,
        players: 1,
        humans: 1,
        slots: 2,
        state: ListingState::Lobby,
        time: 420,
        ringouts: 2,
        hazards: true,
    })
}

fn code_of(address: &str, lobby: u64, fingerprint: &str) -> String {
    let c = DirectCode {
        addresses: vec![address.to_owned()],
        port: 7778,
        fingerprint: fingerprint.to_owned(),
        lobby: format!("l{lobby:020x}"),
    };
    encode_direct_code(&c, true).expect("a code")
}

fn announcement(code: String, members: u32) -> Announcement {
    Announcement { code, info: info(), members, max: 4 }
}

fn packet(code: String, members: u32) -> Vec<u8> {
    encode_announcement(&announcement(code, members)).expect("a packet")
}

fn default_packet() -> Vec<u8> {
    packet(code_of("127.0.0.1", 1, FP), 1)
}

fn listener(own: &str) -> LanListener {
    LanListener::new(ListenerOptions { port: Some(1), own: own.to_owned(), loopback: true, log: None })
}

fn nic(address: &str, netmask: &str, internal: bool) -> Nic {
    Nic { address: address.to_owned(), netmask: netmask.to_owned(), v4: true, internal }
}

#[test]
fn packets() {
    let a = announcement(code_of("127.0.0.1", 1, FP), 2);
    let b = encode_announcement(&a).expect("made");
    assert_eq!(decode_announcement(&b), Some(a.clone()));
    assert!(b.len() < ANNOUNCE_MAX_BYTES);
    // one past the cap is not made
    let big = Announcement { info: "x".repeat(ANNOUNCE_MAX_BYTES), ..a };
    assert_eq!(encode_announcement(&big), None);
    let mut magic = default_packet();
    magic[0] = 0;
    let mut other = default_packet();
    other[4] = 2;
    let cut = default_packet()[..12].to_vec();
    let mut long = default_packet();
    long.resize(ANNOUNCE_MAX_BYTES + 1, 0);
    assert!(decode_announcement(&magic).is_none());
    assert!(decode_announcement(&other).is_none());
    assert!(decode_announcement(&cut).is_none());
    assert!(decode_announcement(&long).is_none());
    let nets = vec![
        ("eth".to_owned(), vec![nic("192.168.1.14", "255.255.255.0", false)]),
        ("vm".to_owned(), vec![nic("10.0.5.1", "255.255.0.0", false)]),
        ("lo".to_owned(), vec![nic("127.0.0.1", "255.0.0.0", true)]),
    ];
    let own = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(broadcast_targets(&own(&["192.168.1.14"]), &nets, false), ["255.255.255.255", "192.168.1.255"]);
    assert_eq!(
        broadcast_targets(&own(&["192.168.1.14", "10.0.5.1"]), &nets, false),
        ["255.255.255.255", "192.168.1.255", "10.0.255.255"]
    );
    assert_eq!(broadcast_targets(&own(&["127.0.0.1"]), &nets, true), ["127.255.255.255"]);
}

#[test]
fn listener_rules() {
    let mut now = 0.0;
    let mut l = listener("ffffffffffffffff");
    assert_eq!(l.take(&default_packet(), "127.0.0.1", now), None);
    assert_eq!(l.heard(now).len(), 1);
    assert_eq!(l.heard(now)[0].members, 1);
    let refusals = [
        l.take(&default_packet(), "8.8.8.8", now),
        l.take(&[1, 2, 3], "127.0.0.2", now),
        l.take(&packet("dnotacode".to_owned(), 1), "127.0.0.2", now),
        l.take(&packet(code_of("127.0.0.1", 2, FP), 1), "127.0.0.2", now),
        l.take(
            &encode_announcement(&Announcement {
                code: code_of("127.0.0.2", 3, FP),
                info: "{\"v\":1}".to_owned(),
                members: 1,
                max: 4,
            })
            .expect("a packet"),
            "127.0.0.2",
            now,
        ),
    ];
    assert_eq!(refusals[0], Some(Dropped::NotLocal));
    assert_eq!(refusals[1], Some(Dropped::NotAnnouncement));
    assert_eq!(refusals[2], Some(Dropped::Code(DirectWhy::Mistyped)));
    assert_eq!(refusals[3], Some(Dropped::NotSender));
    assert_eq!(refusals[4], Some(Dropped::Listing));
    assert_eq!(l.heard(now).len(), 1);
    // its own install's lobby is not kept
    l.take(&packet(code_of("127.0.0.3", 4, "ffffffffffffffff"), 1), "127.0.0.3", now);
    assert_eq!(l.heard(now).len(), 1);
    // a code of two addresses: kept cut down to its sender's; the same lobby from its other address not kept again
    let two = encode_direct_code(
        &DirectCode {
            addresses: vec!["127.0.0.7".to_owned(), "127.0.0.8".to_owned()],
            port: 7778,
            fingerprint: FP.to_owned(),
            lobby: format!("l{}", "7".repeat(20)),
        },
        true,
    )
    .expect("a code");
    assert_eq!(l.take(&packet(two.clone(), 1), "127.0.0.8", now), None);
    assert_eq!(l.take(&packet(two, 1), "127.0.0.7", now), None);
    let heard = l.heard(now);
    assert_eq!(heard.len(), 2);
    let row = heard.iter().find(|h| h.from == "127.0.0.8").expect("the row");
    let cut = decode_direct_code(&row.code, &Local::new(true)).expect("a code");
    assert_eq!(cut.addresses, ["127.0.0.8"]);
    assert_eq!(cut.lobby, format!("l{}", "7".repeat(20)));
    // a lobby heard again is one row, its members the latest
    l.take(&packet(code_of("127.0.0.1", 1, FP), 3), "127.0.0.1", now);
    assert_eq!(l.heard(now).len(), 2);
    assert_eq!(l.heard(now)[0].members, 3);
    // kept up to three announcements missed, gone after
    now += HEARD_FOR_MS;
    assert_eq!(l.heard(now).len(), 2);
    now += 1.0;
    assert_eq!(l.heard(now).len(), 0);
    // the cap per sender
    let mut p = listener("");
    let per: Vec<_> = (0..=SENDER_LOBBIES as u64)
        .map(|i| p.take(&packet(code_of("127.0.0.9", 900 + i, FP), 1), "127.0.0.9", now))
        .collect();
    assert_eq!(p.heard(now).len(), SENDER_LOBBIES);
    assert_eq!(per.last().cloned().flatten(), Some(Dropped::TooManyFromAddress));
    // the cap: lobbies from many senders, each within its rate
    for i in 0..HEARD_MAX + 6 {
        let a = format!("127.0.{}.{}", 1 + (i >> 8), if i & 255 == 0 { 1 } else { i & 255 });
        l.take(&packet(code_of(&a, 100 + i as u64, FP), 1), &a, now);
    }
    let why = l.take(&packet(code_of("127.9.9.9", 999, FP), 1), "127.9.9.9", now);
    assert_eq!(l.heard(now).len(), HEARD_MAX);
    assert_eq!(why, Some(Dropped::TooMany));
    // the rate: SENDER_PACKETS per window from one address, then dropped until the next window
    let mut r = listener("");
    let took = (0..SENDER_PACKETS + 5)
        .filter(|_| r.take(&packet(code_of("127.0.0.5", 500, FP), 1), "127.0.0.5", now).is_none())
        .count();
    let late = r.take(&packet(code_of("127.0.0.5", 500, FP), 1), "127.0.0.5", now);
    now += ANNOUNCE_EVERY_MS;
    let next = r.take(&packet(code_of("127.0.0.5", 501, FP), 1), "127.0.0.5", now);
    assert_eq!(took, SENDER_PACKETS as usize);
    assert_eq!(late, Some(Dropped::Rate));
    assert_eq!(next, None);
    // a sender outside the network is counted against its rate too
    let far: Vec<_> = (0..=SENDER_PACKETS).map(|_| r.take(&default_packet(), "8.8.8.8", now)).collect();
    assert!(far[..far.len() - 1].iter().all(|w| *w == Some(Dropped::NotLocal)));
    assert_eq!(far.last().cloned().flatten(), Some(Dropped::Rate));
}

fn free_udp_port() -> u16 {
    let s = UdpSocket::bind("127.0.0.1:0").expect("a port");
    s.local_addr().expect("an address").port()
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn log_into(lines: &Arc<Mutex<Vec<String>>>) -> Option<LogFn> {
    let lines = lines.clone();
    Some(Box::new(move |l: &str| lines.lock().expect("lines").push(l.to_owned())))
}

/// Two listeners on one port of 127.0.0.1 (reuse-address) and an announcer: both hear it.
#[test]
fn sockets() {
    let port = free_udp_port();
    let t0 = Instant::now();
    let mut a = LanListener::new(ListenerOptions { port: Some(port), own: "a".repeat(16), loopback: true, log: None });
    let mut b = LanListener::new(ListenerOptions { port: Some(port), own: "b".repeat(16), loopback: true, log: None });
    a.start().expect("a listens");
    b.start().expect("b listens");
    let mut ann = LanAnnouncer::new(AnnouncerOptions {
        port: Some(port),
        addresses: vec!["127.0.0.1".to_owned()],
        loopback: true,
        log: None,
        interfaces: None,
    });
    ann.set(Some(&announcement(code_of("127.0.0.1", 7, FP), 1)), ms(t0));
    let mut heard = (0, 0);
    for _ in 0..100 {
        a.pump(ms(t0));
        b.pump(ms(t0));
        heard = (a.heard(ms(t0)).len(), b.heard(ms(t0)).len());
        if heard == (1, 1) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(heard, (1, 1), "two listeners on one port both hear one broadcast");
    ann.close();
    a.close();
    b.close();

    // bad packets from one sender: one log line in its window, none past its rate
    let lines = Arc::new(Mutex::new(Vec::new()));
    let mut c = LanListener::new(ListenerOptions {
        port: Some(port),
        own: String::new(),
        loopback: true,
        log: log_into(&lines),
    });
    c.start().expect("c listens");
    let raw = UdpSocket::bind("127.0.0.1:0").expect("a socket");
    for _ in 0..SENDER_PACKETS + 10 {
        raw.send_to(&[1, 2, 3], ("127.0.0.1", port)).expect("sent");
    }
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(20));
        c.pump(ms(t0));
    }
    let got = lines.lock().expect("lines").clone();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("dropped"), "{got:?}");
    c.close();
}

fn nic4(address: &str, netmask: &str) -> Nic {
    Nic { address: address.to_owned(), netmask: netmask.to_owned(), v4: true, internal: false }
}

/// This PC's interfaces: a home LAN and, with `vpn`, the virtual network adapter `vpn` names.
fn pc(vpn: Option<(&str, &str, &str)>) -> Vec<(String, Vec<Nic>)> {
    let mut out = vec![("Ethernet".to_owned(), vec![nic4("192.168.1.5", "255.255.255.0")])];
    if let Some((name, a, m)) = vpn {
        out.push((name.to_owned(), vec![nic4(a, m)]));
    }
    out
}

/// The address rule (O4): a peer is local in the private and link-local ranges, and inside one of this PC's own
/// networks a code may name (a virtual network adapter's, 100.64/10; prefix /8 or longer); each virtual network's range
/// on and off an interface of this PC.
#[test]
fn an_address_is_local_on_one_of_this_pcs_networks() {
    use mvs_net::lan::direct_code::{local_address, pick_addresses};
    // (the tool, its adapter, this PC's address and netmask on it, a friend's address on the same network)
    let tools = [
        ("Tailscale", "Tailscale", "100.101.102.103", "255.192.0.0", "100.70.1.2"),
        ("ZeroTier", "ZeroTier One [8056c2e21c000001]", "172.27.4.4", "255.255.0.0", "172.27.9.9"),
        ("ZeroTier, a public range", "ZeroTier One [8056c2e21c000002]", "28.1.2.3", "255.255.255.0", "28.1.2.40"),
        ("Radmin VPN", "Radmin VPN", "26.10.20.30", "255.0.0.0", "26.200.1.1"),
        ("Hamachi", "Hamachi", "25.1.2.3", "255.0.0.0", "25.77.8.9"),
    ];
    for (tool, adapter, mine, mask, friend) in tools {
        let on = Local::of(&pc(Some((adapter, mine, mask))), false);
        let off = Local::of(&pc(None), false);
        let private = friend.starts_with("172.");
        assert!(local_address(friend, &on), "{tool}: a friend on the network, on its interface");
        assert!(local_address(&format!("::ffff:{friend}"), &on), "{tool}: as IPv4 in IPv6");
        assert_eq!(local_address(friend, &off), private, "{tool}: off the interface, the private ranges alone");
        // the host writes its address on the tool's network (after the LAN's), and the friend reads it
        let picked = pick_addresses(&pc(Some((adapter, mine, mask))), false);
        assert_eq!(picked, ["192.168.1.5", mine], "{tool}: picked");
        let c = DirectCode {
            addresses: vec![mine.to_owned()],
            port: 7777,
            fingerprint: FP.into(),
            lobby: format!("l{}", "7".repeat(20)),
        };
        let code = encode_direct_code(&c, false).expect("a code");
        let friend_pc = Local::of(&pc(Some((adapter, friend, mask))), false);
        assert_eq!(decode_direct_code(&code, &friend_pc).map(|d| d.addresses), Ok(vec![mine.to_owned()]), "{tool}");
        let expect = if private { Ok(vec![mine.to_owned()]) } else { Err(DirectWhy::Outside) };
        assert_eq!(decode_direct_code(&code, &off).map(|d| d.addresses), expect, "{tool}: a reader off the network");
    }
    // the LAN itself, and what is never local: another subnet's public address, the internet, loopback, a /4
    let home = Local::of(&pc(None), false);
    assert!(local_address("192.168.1.77", &home) && local_address("10.4.4.4", &home));
    assert!(!local_address("8.8.8.8", &home) && !local_address("127.0.0.1", &home));
    let wide = Local::of(&pc(Some(("ZeroTier One [a]", "32.0.0.1", "240.0.0.0"))), false);
    assert!(!local_address("40.1.1.1", &wide), "a netmask shorter than /8 adds nothing");
    let gapped = Local::of(&pc(Some(("ZeroTier One [b]", "33.0.0.1", "255.0.255.0"))), false);
    assert!(!local_address("33.9.0.1", &gapped), "a netmask with a gap adds nothing");
    // Tailscale's adapter: all of 100.64/10, whatever netmask it reports
    let ts32 = Local::of(&pc(Some(("Tailscale", "100.101.102.103", "255.255.255.255"))), false);
    assert!(local_address("100.70.1.2", &ts32) && !local_address("100.128.0.1", &ts32));
    // a PC on the internet directly, or a hotspot's public address (a public address on a plain adapter): its subnet
    // is not local, and the address is never written in a code
    let direct = vec![
        ("Ethernet".to_owned(), vec![nic4("203.0.113.5", "255.255.255.0")]),
        ("Wi-Fi".to_owned(), vec![nic4("198.51.100.7", "255.255.255.0")]),
    ];
    assert_eq!(pick_addresses(&direct, false), Vec::<String>::new(), "a public address of a plain adapter");
    assert!(!local_address("203.0.113.77", &Local::of(&direct, false)), "a plain adapter's public subnet");
    let hotspot =
        vec![("Wi-Fi".to_owned(), vec![nic4("192.168.43.2", "255.255.255.0"), nic4("31.4.5.6", "255.0.0.0")])];
    assert_eq!(pick_addresses(&hotspot, false), ["192.168.43.2"]);
    assert!(!local_address("31.9.9.9", &Local::of(&hotspot, false)), "a hotspot's public /8");
    // a carrier-grade NAT address is written from any adapter (Tailscale's range), after the private ones
    let cgnat = vec![("Ethernet".to_owned(), vec![nic4("100.64.0.9", "255.192.0.0"), nic4("10.0.0.2", "255.0.0.0")])];
    assert_eq!(pick_addresses(&cgnat, false), ["10.0.0.2", "100.64.0.9"]);
    // Wi-Fi, WSL 2 and Radmin: the LAN's and Radmin's take the code's two slots, the VM's private address after them
    let three = vec![
        ("Wi-Fi".to_owned(), vec![nic4("192.168.1.5", "255.255.255.0")]),
        ("vEthernet (WSL)".to_owned(), vec![nic4("172.20.0.1", "255.255.240.0")]),
        ("Radmin VPN".to_owned(), vec![nic4("26.10.20.30", "255.0.0.0")]),
    ];
    assert_eq!(pick_addresses(&three, false), ["192.168.1.5", "26.10.20.30", "172.20.0.1"]);
}

/// A code naming the host's LAN and virtual network addresses: each reader keeps the ones it can reach, and only a
/// reader on neither network is told the code is outside.
#[test]
fn a_mixed_code_is_read_for_the_addresses_the_reader_takes() {
    let lobby = format!("l{}", "7".repeat(20));
    let code = |addresses: &[&str]| {
        let c = DirectCode {
            addresses: addresses.iter().map(|a| (*a).to_owned()).collect(),
            port: 7777,
            fingerprint: FP.into(),
            lobby: lobby.clone(),
        };
        encode_direct_code(&c, false).expect("a code")
    };
    let read = |c: &str, local: &Local| decode_direct_code(c, local).map(|d| d.addresses);
    let lan_only = Local::of(&pc(None), false);
    let vpn_only = Local::of(&vec![("Radmin VPN".to_owned(), vec![nic4("26.200.1.1", "255.0.0.0")])], false);
    let mixed = code(&["192.168.1.5", "26.10.20.30"]);
    assert_eq!(read(&mixed, &lan_only), Ok(vec!["192.168.1.5".to_owned()]), "a friend on the LAN alone");
    // (a private address is local to every reader: the VPN-only one tries it too, then the VPN's)
    assert_eq!(read(&mixed, &vpn_only), Ok(vec!["192.168.1.5".to_owned(), "26.10.20.30".to_owned()]));
    let vpns = code(&["26.10.20.30", "100.70.1.2"]);
    assert_eq!(read(&vpns, &vpn_only), Ok(vec!["26.10.20.30".to_owned()]), "Radmin's, not Tailscale's");
    assert_eq!(read(&vpns, &lan_only), Err(DirectWhy::Outside), "on neither network");
}
