//! The program's online parts made and fed: the net
//! service from the online config, the hub over its page, what the hub reads of the settings, this game's loadout,
//! and a started online match as the program's match.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use mvs_net::backend::eos::{EosFiles, EosOptions};
use mvs_net::link::relay::NetFaults;
use mvs_net::service::api::{
    DirectOptions, EpicBackend, EpicMissing, LanOptions, OnlineBackends, RelayOptions, RelayPin,
};
use mvs_net::service::{NetHandle, ServiceOptions};
use mvs_protocol::loadout::NetLoadout;
use mvs_protocol::online::{BadgeRef, CosmeticRef};
use mvs_protocol::relay_wire::KnownRelay;
use mvs_settings::Settings;
use mvs_settings::online::{OnlineConfig, OnlineSetting};
use mvs_ui::bundle::Bundle;
use mvs_ui::menu::lobby::loadout::{
    BotPool, asset_name, character_slug, local_badges, local_ring_out, local_taunts, ported_setup,
};

use super::hub::{NetMatchStart, OnlineHub, Prefs};
use super::net_match::NetMatch;
use super::port::NetPort;
use super::tables::Tables;
use crate::app::{MatchKind, MatchStart};

/// The product name the relays and the direct hosts are told.
pub(crate) const PRODUCT: &str = "mvs";
/// The product name EOS is told.
const EOS_PRODUCT: &str = "MVS Classic";

/// Where the EOS SDK's DLL and the product's credentials are: those MVS_EOS_DLL and
/// MVS_EOS_CREDENTIALS name when both are files, else the release folder's beside the exe (`mvs_release::EOS_DLL`,
/// `EOS_CREDENTIALS`: a release built with the SDK packs both); None when neither pair is whole (online play through
/// Epic is off).
// Deviation: the exe's folder alone was taken in a release and the environment alone in a dev run; here the
// environment pair wins in any build, and a half pair (one variable unset or not a file) falls through to the exe's
// folder without a word.
pub fn eos_files() -> Option<(PathBuf, PathBuf)> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if let (Some(dll), Some(credentials)) = (env("MVS_EOS_DLL"), env("MVS_EOS_CREDENTIALS"))
        && dll.is_file()
        && credentials.is_file()
    {
        return Some((dll, credentials));
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let dll = dir.join(mvs_release::EOS_DLL);
    let credentials = mvs_release::EOS_CREDENTIALS.split('/').fold(dir, |p, part| p.join(part));
    (dll.is_file() && credentials.is_file()).then_some((dll, credentials))
}

/// Epic's options: the SDK and the product's credentials where `eos_files` finds them, the SDK's
/// cache in the data folder; else not set up. MVS_EOS_FRESH_DEVICE set: a Device ID of its own (a dev run's second
/// player on one PC).
fn epic_of(data: &Path, version: &str) -> EpicBackend {
    match eos_files() {
        Some((dll, credentials)) => EpicBackend {
            options: Some(EosOptions {
                files: EosFiles { dll, credentials, cache: data.join("eos") },
                product: EOS_PRODUCT.to_owned(),
                version: version.to_owned(),
                fresh_device: std::env::var_os("MVS_EOS_FRESH_DEVICE").is_some(),
                spin: false,
            }),
            missing: None,
        },
        None => EpicBackend { options: None, missing: Some(EpicMissing::NotSetUp) },
    }
}

/// The online config as the service takes it: Epic's from `epic_of`; `loopback` keeps every listen and announce
/// on 127.0.0.1 (`--direct-loopback`, the two-exe runs).
/// `dev`: the file named by `--online-config`, whose relay and LAN port win over the player's; else the release's
/// online.json, each field the player set winning. `faults`: the relay client's (dev
/// switch).
pub fn backends_of(
    file: &Path,
    settings: &Settings,
    data: &Path,
    loopback: bool,
    dev: bool,
    faults: NetFaults,
) -> (OnlineBackends, DirectOptions) {
    let read = mvs_settings::online::read_online_config(file, &mut |l| eprintln!("mvs: {l}"));
    let mut c = mvs_settings::online::effective_online(&read, &settings.online);
    if dev {
        // Deviation: the config named on the command line rules where the game connects: its relay over the
        // player's saved one, its LAN port alone. A scripted or dev run never dials a relay the player saved.
        c.relay.clone_from(&read.relay);
        c.relay_id.clone_from(&read.relay_id);
        c.lan = read.lan;
    }
    let pin = pin_of(&c, &settings.online, dev);
    if c.relay.is_some() && pin.is_none() {
        eprintln!("mvs: online: the relay's key is not pinned (no relayId in the online config, no id kept for it)");
    }
    let version = mvs_release::VERSION.to_owned();
    let epic = c.eos.then(|| epic_of(data, &version));
    let key_file = data.join("relay.key");
    let direct =
        DirectOptions { key_file: key_file.clone(), product: PRODUCT.to_owned(), version: version.clone(), loopback };
    let relay = c.relay.clone().map(|address| RelayOptions {
        address,
        key_file,
        product: PRODUCT.to_owned(),
        version,
        host_key: None,
        pin,
        faults,
    });
    let backends = OnlineBackends {
        relay,
        epic,
        lan: c.lan.map(|port| LanOptions { direct: direct.clone(), port }),
        allow_direct: c.direct,
        default_relay: read.relay,
        known_relays: settings.online.known_relays.clone(),
    };
    (backends, direct)
}

/// The key the relay in use must prove: online.json's relayId for the release's relay (`c` the effective config,
/// whose relay_id is the file's only for its relay), else the id kept for a relay the player said yes to (not in a dev
/// config's run, whose relay is the config's alone); None: any key (no relayId, a relay never signed in to).
pub fn pin_of(c: &OnlineConfig, player: &OnlineSetting, dev: bool) -> Option<RelayPin> {
    let address = c.relay.as_deref()?;
    if let Some(id) = &c.relay_id {
        return Some(RelayPin { id: id.clone(), release: true });
    }
    if dev {
        return None;
    }
    let id = player.known_relay(address)?.id.clone()?;
    Some(RelayPin { id, release: false })
}

/// The net service on its thread and the hub over its page: a normal run's on the release's online.json (`dev`
/// false), `--online-config <file>`'s (`dev`, backends_of). `lag` and `faults`: the dev switches' delay of the match's
/// packets and faults of the relay client's link.
#[allow(clippy::too_many_arguments, reason = "the run's online settings, each its own")]
pub fn start(
    file: &Path,
    settings: &Settings,
    data: &Path,
    loopback: bool,
    dev: bool,
    compat: String,
    lag: f64,
    faults: NetFaults,
) -> (NetHandle, Rc<RefCell<OnlineHub>>) {
    let epoch = std::time::Instant::now();
    // (a dev config's run, the two-exe scripts: the service's own lines on stderr)
    let log = dev.then(|| Box::new(|l: &str| eprintln!("mvs: net: {l}")) as mvs_host::HostLog);
    // (the player's run tells the launcher its sign-ins; a dev run's data folder is its own and has no launcher)
    let state_file = (!dev).then(|| data.join(mvs_net::service::api::NET_STATE_FILE));
    let handle = mvs_net::service::spawn(ServiceOptions { epoch, log, state_file, dev, ..ServiceOptions::default() });
    let (b, d) = backends_of(file, settings, data, loopback, dev, faults);
    let port = NetPort::new(Some(Box::new(handle.page())));
    let mut hub = OnlineHub::new(port, Some(data.join(super::recent::FILE)), compat, Tables::default(), lag);
    hub.epoch = epoch;
    if dev {
        // (a dev config's run: no gate, its backends at once)
        handle.backends(b, d);
        hub.dev_on = std::env::var("MVS_DEV_SESSION_ON").ok().and_then(|t| dev_session_on(&t));
    } else {
        // (a player's run: the backends held back until the gate's checks let them, apply_gate)
        let release = mvs_settings::online::read_online_config(file, &mut |_| {});
        *HELD.lock().unwrap_or_else(PoisonError::into_inner) = Some(Held {
            backends: b,
            direct: d,
            on: false,
            release,
            file: file.to_path_buf(),
            data: data.to_path_buf(),
            loopback,
            faults,
            setting: settings.online.clone(),
            stamp: settings_stamp(data),
            looked: None,
        });
        super::gate::arm(&mut hub, mvs_release::FEED, mvs_release::VERSION);
    }
    (handle, Rc::new(RefCell::new(hub)))
}

/// (dev) MVS_DEV_SESSION_ON of a dev config's run: the way the menus are told an open session is on, whatever the
/// service says (the lobby's connection line and the ping line of each way, for their pictures): `relay`, `epic`,
/// `lan:<address:port>`, `local:version` (the relay of another version) or `local:off` (the menus' line's reason).
fn dev_session_on(text: &str) -> Option<mvs_net::service::api::SessionOn> {
    use mvs_net::service::api::{LocalWhy, SessionOn, StatusWhy};
    let on = match text {
        "relay" => SessionOn::Relay,
        "epic" => SessionOn::Epic,
        "local:version" => SessionOn::Local { why: LocalWhy::RelayVersion },
        "local:off" => SessionOn::Local { why: LocalWhy::Status { why: StatusWhy::Off { backends: Vec::new() } } },
        t => SessionOn::Lan { at: t.strip_prefix("lan:")?.to_owned() },
    };
    Some(on)
}

/// A player's run's backends (glue::start), sent to the service while no gate is set.
#[derive(Debug)]
struct Held {
    backends: OnlineBackends,
    direct: DirectOptions,
    /// Last sent: the backends (true) or none (false; at the start nothing is sent).
    on: bool,
    /// online.json's values (keep_known_relays: the release's relay is never kept).
    release: OnlineConfig,
    /// What the backends are built from again when the launcher changes the online settings (follow_settings):
    /// online.json, the data folder, start's switches.
    file: PathBuf,
    data: PathBuf,
    loopback: bool,
    faults: NetFaults,
    /// The player's online setting the backends were last built from, the settings file's time then, and when the
    /// file was last looked at.
    setting: OnlineSetting,
    stamp: Option<SystemTime>,
    looked: Option<Instant>,
}

/// How often the menus look at the settings file for a change the launcher made.
const SETTINGS_EVERY: Duration = Duration::from_secs(1);

/// The settings file's last write (None: none, or not readable).
fn settings_stamp(data: &Path) -> Option<SystemTime> {
    std::fs::metadata(data.join(mvs_settings::FILE)).and_then(|m| m.modified()).ok()
}

/// The run's held backends (one game a process; None: a dev config's run, whose backends went at once).
static HELD: Mutex<Option<Held>> = Mutex::new(None);

/// The backends with every one off and the rest of `b` kept (the relays known, the release's default).
fn none_of(b: &OnlineBackends) -> OnlineBackends {
    OnlineBackends { relay: None, epic: None, lan: None, ..b.clone() }
}

/// Each frame of the menus, after the gate is polled: while a gate is set the online backends are off (none signs in
/// before the update, the import or the self check let it), else on. A dev config's run is left as it is.
pub fn apply_gate(net: &NetHandle, gate: Option<&super::gate::Gate>) {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(h) = held.as_mut() else { return };
    let on = gate.is_none();
    if on == h.on {
        return;
    }
    h.on = on;
    let b = if on { h.backends.clone() } else { none_of(&h.backends) };
    if on {
        eprintln!("mvs: online: no gate: the online backends start");
    } else {
        eprintln!("mvs: online: {gate:?}: the online backends stop");
    }
    net.backends(b, h.direct.clone());
}

/// Each frame of the menus, at most every SETTINGS_EVERY: when the settings file changed (the launcher's Online tab
/// writes it), the player's online setting read again and the backends built from it again; when they changed, the
/// service takes them at once while no custom lobby or queue is open, else once it closes
/// (`NetHandle::backends_when_free`). While a gate is set they are only kept: `apply_gate` sends them when it lifts.
/// A dev config's run is left as it is (no HELD).
pub fn follow_settings(net: &NetHandle) {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(h) = held.as_mut() else { return };
    if h.looked.is_some_and(|t| t.elapsed() < SETTINGS_EVERY) {
        return;
    }
    h.looked = Some(Instant::now());
    if h.reread() && h.on {
        eprintln!("mvs: online: the online settings changed in the settings file: the backends follow");
        net.backends_when_free(h.backends.clone(), h.direct.clone());
    }
}

impl Held {
    /// The settings file read again when it changed since the last look, and the backends built again from its
    /// online setting: true when they changed.
    fn reread(&mut self) -> bool {
        let stamp = settings_stamp(&self.data);
        if stamp == self.stamp {
            return false;
        }
        self.stamp = stamp;
        let mut disk = Settings::load(&self.data);
        if disk.online == self.setting {
            return false;
        }
        self.setting = disk.online.clone();
        // (the relays this run said yes to and has not saved yet stay; the file's id wins for a relay both know, the
        // launcher's trust of a new key)
        for k in &self.backends.known_relays {
            if disk.online.known_relay(&k.address).is_none() {
                disk.online.known_relays.push(k.clone());
            }
        }
        let (b, d) = backends_of(&self.file, &disk, &self.data, self.loopback, false, self.faults);
        if b == self.backends && d == self.direct {
            return false;
        }
        self.backends = b;
        self.direct = d;
        true
    }
}

/// Each frame of the menus: the relays the player said yes to since the last frame, and the ids they proved, kept in
/// Settings.online (the game saves the settings when they change) and in the held backends (a gate going off and on
/// configures the service with them). A dev config's run keeps none: its relay is the config's, and its data folder
/// its own.
pub fn keep_known_relays(net: &NetHandle, settings: &mut Settings) {
    let found = net.relays_known();
    if !found.is_empty() {
        keep_held(settings, found);
    }
}

/// `found` kept in the settings and the held backends; nothing in a dev config's run (no HELD).
fn keep_held(settings: &mut Settings, found: Vec<KnownRelay>) {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(h) = held.as_mut() {
        keep_found(&mut settings.online, &h.release, found);
        h.backends.known_relays.clone_from(&settings.online.known_relays);
    }
}

/// `found` kept in the player's setting (`OnlineSetting::know_relay`), a log line for each one that changed it.
fn keep_found(player: &mut OnlineSetting, release: &OnlineConfig, found: Vec<KnownRelay>) {
    for k in found {
        let line = format!("mvs: online: relay {} kept, key id {}", k.address, k.id.as_deref().unwrap_or("not seen"));
        if player.know_relay(release, k) {
            eprintln!("{line}");
        }
    }
}

/// (dev) The hub with no net service (`--queue-fake` without `--online-config`): online play off, the fake queue
/// on.
pub fn start_offline(data: &Path) -> Rc<RefCell<OnlineHub>> {
    let hub =
        OnlineHub::new(NetPort::new(None), Some(data.join(super::recent::FILE)), String::new(), Tables::default(), 0.0);
    Rc::new(RefCell::new(hub))
}

/// The menus' tables bound to the hub once the menu data is in.
pub fn bind_tables(hub: &mut OnlineHub, bundle: &Bundle) {
    let Some(menu) = bundle.menu.as_ref() else { return };
    let pool = BotPool::of(&menu.cosmetics);
    if let Some(f) = &mut hub.fake_lobbies {
        f.icons = super::fake_lobbies::icons_of(&menu.cosmetics);
    }
    hub.tables = Tables::of(menu, pool, &|map| {
        menu.maps.get(map).and_then(|m| m.levels.first()).is_some_and(|l| bundle.stage_of_level(l).is_some())
    });
}

/// What the hub reads of the settings each frame.
pub fn prefs_of(s: &Settings) -> Prefs {
    let icon = &s.hydra.profile_icon;
    Prefs {
        name: s.profile.name.clone(),
        icon: if icon.slug.is_empty() { String::new() } else { asset_name(&icon.path).to_owned() },
        listed: s.ui.show_in_online_list,
        invites: s.ui.invites,
    }
}

/// This game's taunts for a character, its ring-out, banner, badges and icon.
/// `count` a badge's count by its catalogue item.
pub fn local_net_loadout(
    s: &Settings,
    bundle: &Bundle,
    character: &str,
    count: &mut dyn FnMut(&mvs_ui::bundle::CosmeticItem) -> f64,
) -> Option<NetLoadout> {
    let menu = bundle.menu.as_ref()?;
    let h = &s.hydra;
    let r = |a: &mvs_settings::AssetRef| CosmeticRef {
        slug: a.slug.clone(),
        asset: if a.slug.is_empty() { String::new() } else { asset_name(&a.path).to_owned() },
    };
    let slug = character_slug(&menu.roster, character);
    let badges = local_badges(h, &menu.cosmetics, count)
        .into_iter()
        .zip(&h.stat_trackers)
        .map(|(b, a)| {
            // (a count is whole and small)
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let count = b.count.max(0.0) as u32;
            BadgeRef { slug: b.slug, asset: r(a).asset, count }
        })
        .collect();
    Some(NetLoadout {
        taunts: local_taunts(h, slug).to_vec(),
        ring_out: local_ring_out(h),
        banner: r(&h.banner),
        badges,
        icon: r(&h.profile_icon),
        input: input_prefs_wire(&s.input, 0),
    })
}

/// localInputPrefs: a local player's fight-read preferences as the getters return
/// them, only the values off their defaults; None when every one is its default.
pub fn input_prefs_wire(s: &mvs_settings::InputSettings, player: usize) -> Option<mvs_protocol::setup::InputPrefsWire> {
    let p = s.players.get(player)?;
    let cap = p.buffer_cap();
    let w = mvs_protocol::setup::InputPrefsWire {
        fastfall: (p.fastfall_type == 1).then_some(1),
        double_crouch_jump_down: (!p.double_crouch_jump_down).then_some(false),
        ignore_right_stick_diagonals: p.ignore_right_stick_diagonals.then_some(true),
        swapped_ground: p.swapped_attack_style_ground.then_some(true),
        swapped_air: p.swapped_attack_style_air.then_some(true),
        input_buffer_cap: u32::try_from(cap).ok().filter(|&c| c <= 30),
        input_buffer_hold: p.input_buffer_hold.then_some(true),
        attack_picks_up_items: (!p.attack_picks_up_items).then_some(false),
    };
    (w != mvs_protocol::setup::InputPrefsWire::default()).then_some(w)
}

/// The sim's set-up of the wire's (the MatchSetup as the net sends it): the trace reader's fields, then each
/// player's team, start, starting damage, ring-out, banner and input preferences, which a trace's set-up never holds.
fn setup_of_wire(w: &mvs_protocol::setup::MatchSetupWire) -> Result<mvs_sim::setup::MatchSetup, String> {
    const PLAYER_ONLY: [&str; 6] = ["team", "start", "startingDamage", "ringOut", "banner", "input"];
    let mut v = serde_json::to_value(w).map_err(|e| e.to_string())?;
    if let Some(ps) = v.get_mut("players").and_then(serde_json::Value::as_array_mut) {
        for p in ps.iter_mut().filter_map(serde_json::Value::as_object_mut) {
            for k in PLAYER_ONLY {
                p.remove(k);
            }
        }
    }
    let mut setup = mvs_sim_trace::setup_of(&v)?;
    for (p, wp) in setup.players.iter_mut().zip(&w.players) {
        p.team = wp.team.and_then(|t| u8::try_from(t).ok());
        p.start = wp.start.and_then(|t| u8::try_from(t).ok());
        p.starting_damage = wp.starting_damage.map(mvs_sim::math::narrow);
        p.ring_out.clone_from(&wp.ring_out);
        p.banner.clone_from(&wp.banner);
        if let Some(i) = &wp.input {
            let d = &mut p.input;
            d.fastfall = i.fastfall.unwrap_or(d.fastfall);
            d.double_crouch_jump_down = i.double_crouch_jump_down.unwrap_or(d.double_crouch_jump_down);
            d.ignore_right_stick_diagonals = i.ignore_right_stick_diagonals.unwrap_or(d.ignore_right_stick_diagonals);
            d.swapped_ground = i.swapped_ground.unwrap_or(d.swapped_ground);
            d.swapped_air = i.swapped_air.unwrap_or(d.swapped_air);
            d.input_buffer_cap = i.input_buffer_cap.and_then(|c| u16::try_from(c).ok()).unwrap_or(d.input_buffer_cap);
            d.input_buffer_hold = i.input_buffer_hold.unwrap_or(d.input_buffer_hold);
            d.attack_picks_up_items = i.attack_picks_up_items.unwrap_or(d.attack_picks_up_items);
        }
    }
    Ok(setup)
}

/// A started online match as the program's: the set-up with the sent perks, kept to
/// the fighters the port draws; a match with no other human (`local`) runs as this game's alone. None: a set-up that
/// does not read.
pub fn match_start_of(
    hub: &Rc<RefCell<OnlineHub>>,
    m: &NetMatchStart,
    drawable: &dyn Fn(&str) -> bool,
    feed: Option<String>,
) -> Option<MatchStart> {
    let s = &m.start;
    let mut setup = match setup_of_wire(&s.setup) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mvs: online: the match's set-up: {e}");
            hub.borrow_mut().match_inbox(false);
            return None;
        }
    };
    // (each slot's sent perks, its empty ones dropped)
    for (i, p) in setup.players.iter_mut().enumerate() {
        p.perks = mvs_sim::setup::perk_list(s.perks.get(i).map_or(&[][..], Vec::as_slice));
    }
    ported_setup(&mut setup, drawable);
    let bots: Vec<bool> = setup.players.iter().map(|p| p.bot).collect();
    // (a local match, no other human: played on this game alone, its result and match.leave sent at its end,
    // OnlineHub::local_over)
    let kind = if s.local {
        hub.borrow_mut().match_inbox(false);
        hub.borrow_mut().local = Some(super::hub::LocalMatch { queue: m.queue.is_some(), map: s.map.clone() });
        MatchKind::Default
    } else {
        MatchKind::Online {
            net: Some(Box::new(super::Online::Net(Box::new(NetMatch::new(hub.clone(), m, &bots, feed))))),
        }
    };
    // (the World's seed: the server's u32, as the sim's i32 bits)
    #[allow(clippy::cast_possible_wrap)]
    let seed = s.seed as i32;
    let mut start = MatchStart::new(setup, kind, seed);
    start.me = s.slot as usize;
    start.usernames.clone_from(&s.usernames);
    start.map = (!s.map.is_empty()).then(|| s.map.clone());
    Some(start)
}

/// cyrb53: a 53-bit string hash over its UTF-16 units, 14 hex digits.
fn hash53(s: &str) -> String {
    let (mut h1, mut h2) = (0xdead_beef_u32, 0x41c6_ce57_u32);
    for c in s.encode_utf16() {
        let c = u32::from(c);
        h1 = (h1 ^ c).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ c).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507) ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507) ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    format!("{:014x}", (u64::from(h2 & 0x1f_ffff) << 32) + u64::from(h1))
}

/// The compatibility id: what must be equal for two games to resolve the server's choices alike and
/// run the same simulation. Deviation: its parts are the store's build id, the exe's version
/// with the sim's numerics version, each kind's importer version and (the port's own) a hash of the shipped custom
/// maps' layouts, in that order; a hash of them, 'c' and 14 hex digits.
pub fn compat_id(m: &mvs_store::Manifest) -> String {
    let mut kinds: Vec<(&String, u64)> = m.kinds.iter().map(|(k, r)| (k, r.version)).collect();
    kinds.sort();
    let kinds: Vec<String> = kinds.iter().map(|(k, v)| format!("{k}{v}")).collect();
    // (the shipped custom maps' layouts: two games play a custom map alike only with the same layouts)
    let layouts = hash53(&mvs_stage::fingerprint());
    let parts =
        format!("{}|{}n{}|{}|l{layouts}", m.build_id, mvs_release::VERSION, mvs_sim::NUMERICS_VERSION, kinds.join(","));
    format!("c{}", hash53(&parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID_A: &str = "r0123456789abcdef0123456789abcdef";
    const ID_B: &str = "rfedcba9876543210fedcba9876543210";

    fn known(a: &str, id: Option<&str>) -> KnownRelay {
        KnownRelay { address: a.into(), id: id.map(str::to_string) }
    }

    /// The launcher's change of the online settings in the settings file: the backends built again from it (the
    /// relay switched off: none; the address kept), nothing for a change of another setting or none, and a relay this
    /// run said yes to and has not saved is kept.
    #[test]
    fn the_backends_follow_the_settings_file() {
        let dir = std::env::temp_dir().join(format!("mvs-glue-follow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("made");
        let file = dir.join("online.json");
        std::fs::write(&file, r#"{ "relay": "relay.example", "eos": false, "lan": null, "direct": false }"#)
            .expect("written");
        let mut settings = Settings::default();
        settings.save(&dir).expect("saved");
        let (backends, direct) = backends_of(&file, &settings, &dir, true, false, NetFaults::default());
        let mut h = Held {
            backends,
            direct,
            on: true,
            release: mvs_settings::online::read_online_config(&file, &mut |_| {}),
            file,
            data: dir.clone(),
            loopback: true,
            faults: NetFaults::default(),
            setting: settings.online.clone(),
            stamp: settings_stamp(&dir),
            looked: None,
        };
        assert!(h.backends.relay.is_some());
        assert!(!h.reread(), "nothing written");
        // (the file's time may not move between two writes this close: each look below forgets it)
        let found = known("found.example:9000", Some(ID_A));
        h.backends.known_relays.push(found.clone());
        settings.ui.invites = !settings.ui.invites;
        settings.save(&dir).expect("saved");
        h.stamp = None;
        assert!(!h.reread(), "another setting");
        settings.online.relay_on = Some(false);
        settings.save(&dir).expect("saved");
        h.stamp = None;
        assert!(h.reread(), "the relay switched off");
        assert_eq!(h.backends.relay, None);
        assert_eq!(h.backends.known_relays, vec![found]);
        h.stamp = None;
        assert!(!h.reread(), "the same again");
        settings.online.relay_on = None;
        settings.save(&dir).expect("saved");
        h.stamp = None;
        assert!(h.reread(), "on again");
        assert_eq!(h.backends.relay.as_ref().map(|r| r.address.as_str()), Some("relay.example"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A custom lobby's Chaos Wheel reaches the sim as the rule the dev switch gives.
    #[test]
    fn a_lobbys_wheel_is_the_sims_rule() {
        let wire = mvs_protocol::setup::MatchSetupWire {
            wheel: Some(mvs_protocol::lobby_rules::wheel_rule()),
            ..Default::default()
        };
        assert_eq!(setup_of_wire(&wire).expect("the set-up reads").wheel, Some(crate::wheel::whole_pool()));
    }

    /// The pin: the release's relay with its relayId, another relay with the id kept for it, else none.
    #[test]
    fn the_relay_is_pinned_to_the_id_expected() {
        let file = OnlineConfig {
            relay: Some("relay.example".into()),
            relay_id: Some(ID_A.into()),
            ..OnlineConfig::default()
        };
        let mut player = OnlineSetting {
            known_relays: vec![known("mine.example:9000", Some(ID_B)), known("old.example", None)],
            ..OnlineSetting::default()
        };
        let pin = |player: &OnlineSetting, dev: bool| {
            pin_of(&mvs_settings::online::effective_online(&file, player), player, dev)
        };
        assert_eq!(pin(&player, false), Some(RelayPin { id: ID_A.into(), release: true }));
        player.relay = Some(Some("RELAY.example:7777".into()));
        assert_eq!(pin(&player, false), Some(RelayPin { id: ID_A.into(), release: true }));
        player.relay = Some(Some("Mine.example:9000".into()));
        assert_eq!(pin(&player, false), Some(RelayPin { id: ID_B.into(), release: false }));
        assert_eq!(pin(&player, true), None);
        player.relay = Some(Some("old.example".into()));
        assert_eq!(pin(&player, false), None);
        player.relay = Some(Some("new.example".into()));
        assert_eq!(pin(&player, false), None);
        player.relay = Some(None);
        assert_eq!(pin(&player, false), None);
        // no relayId in the file: the release's relay is not pinned
        let unpinned = OnlineConfig { relay_id: None, ..file.clone() };
        let p = OnlineSetting::default();
        assert_eq!(pin_of(&mvs_settings::online::effective_online(&unpinned, &p), &p, false), None);
    }

    /// The relays the service says yes to reach the player's setting: a new one kept with its id, a known one's id
    /// replaced, the release's own not kept.
    #[test]
    fn the_relays_said_yes_to_are_kept() {
        let file = OnlineConfig {
            relay: Some("relay.example".into()),
            relay_id: Some(ID_A.into()),
            ..OnlineConfig::default()
        };
        let mut player = OnlineSetting { known_relays: vec![known("old.example", None)], ..OnlineSetting::default() };
        keep_found(
            &mut player,
            &file,
            vec![
                known("new.example:9000", Some(ID_A)),
                known("OLD.example:7777", Some(ID_B)),
                known("relay.example", Some(ID_B)),
            ],
        );
        assert_eq!(player.known_relays, vec![known("old.example", Some(ID_B)), known("new.example:9000", Some(ID_A))]);
        keep_found(&mut player, &file, vec![known("new.example:9000", Some(ID_B))]);
        assert_eq!(player.known_relays[1], known("new.example:9000", Some(ID_B)));
    }

    /// A dev config's run: pinned to its own relayId, and the relays it says yes to never kept (no HELD: glue::start
    /// sets it only in a player's run).
    #[test]
    fn a_dev_run_pins_its_config_and_keeps_nothing() {
        let dir = std::env::temp_dir().join(format!("mvs-glue-dev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("made");
        let file = dir.join("online.json");
        let config = format!(
            r#"{{ "relay": "127.0.0.1:9000", "relayId": "{ID_A}", "eos": false, "lan": null, "direct": false }}"#
        );
        std::fs::write(&file, config).expect("written");
        let mut settings = Settings::default();
        settings.online.relay = Some(Some("mine.example".into()));
        settings.online.known_relays = vec![known("127.0.0.1:9000", Some(ID_B))];
        let (b, _) = backends_of(&file, &settings, &dir, true, true, NetFaults::default());
        let relay = b.relay.expect("the config's relay");
        assert_eq!(relay.address, "127.0.0.1:9000");
        assert_eq!(relay.pin, Some(RelayPin { id: ID_A.into(), release: true }));
        // the player's run on the same file: the player's relay, pinned to the id kept for it when there is one
        let (b, _) = backends_of(&file, &settings, &dir, true, false, NetFaults::default());
        assert_eq!(b.relay.expect("the player's relay").pin, None);
        assert!(HELD.lock().unwrap_or_else(PoisonError::into_inner).is_none());
        keep_held(&mut settings, vec![known("new.example", Some(ID_A))]);
        assert_eq!(settings.online.known_relays, vec![known("127.0.0.1:9000", Some(ID_B))]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash53_as_the_page() {
        // (hash53 of 'b1|dev|a1,b2' and '')
        assert_eq!(hash53("b1|dev|a1,b2"), "1074a5b9be7604");
        assert_eq!(hash53(""), "0bdcb81aee8d83");
    }
}
