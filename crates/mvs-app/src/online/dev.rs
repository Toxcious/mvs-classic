//! The dev runs' online match: the command line's way into one (`--relay`, `--host`, `--join`, `--key`)
//! and the tick source that steps the World through a rollback session (`mvs_net::session`) on the net thread's link.
//!
//! Deviation: no host core yet. The two players meet in a relay lobby the host makes and the guest joins by its id,
//! and agree on the match over channel 0 by a few messages of the port's own ([`Say`]): the host's set-up (the seed,
//! the slots), each side's load, the host's go with the referee's id (the host's net thread's) and START_AHEAD_MS,
//! the game's start gate, and the guest's end. The match is the
//! default Versus 1v1 on the training room.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mvs_net::link::MatchLink;
use mvs_net::link::relay::key::RelayKey;
use mvs_net::link::relay::{CHANNEL_MESSAGES, ClientOptions, NetFaults, RelayClient, RelayEvent, RelayMatchLink};
use mvs_net::session::{Session, SessionConfig, SlotKind};
use mvs_protocol::match_wire::{PingWindow, START_AHEAD_MS};
use mvs_protocol::referee::SlotNews;
use mvs_sim::ai::bt::BtBot;
use mvs_sim::{Event, InputState, World};
use mvs_sim_trace::Book;
use mvs_trace::{Header, Trace, TraceWriter};
use serde::{Deserialize, Serialize};

use crate::app::{MatchKind, MatchStart};
use crate::run::default_setup;
use crate::{Error, Result};

/// The relay lobbies' bucket of these dev matches.
const BUCKET: &str = "mvs-dev";
/// The guest's hello goes this often until the host's set-up comes (ms).
const HI_EVERY_MS: f64 = 500.0;
/// The guest pings the host this often while it waits for the go (the start gate's one-way time).
const PING_EVERY_MS: f64 = 100.0;
/// The lobby's meeting waits this long at the most (ms).
const MEET_MS: f64 = 600_000.0;
/// At its end the host's net thread (the match's referee, or its owner on the relay) stays this long at the most for
/// the guest's own end (ms).
const DONE_WAIT_MS: f64 = 20_000.0;
/// The start gate's time limit from the match's load.
const LOAD_MS: f64 = 300_000.0;
/// The state hashes' frames kept for a desync's report.
const KEEP_FRAMES: usize = 16;

/// The command line's online options. Online play starts from them alone (no menu yet): a Versus 1v1 on the relay at
/// `relay`, this player hosting a lobby or joining one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineOptions {
    /// `--relay <addr>`: the relay, `host[:port]` (`mvs_protocol::relay_wire::parse_address`).
    pub relay: String,
    /// `--host`: this player makes the lobby and runs the match's host core.
    pub host: bool,
    /// `--join <lobby>`: the lobby to join.
    pub join: Option<String>,
    /// `--key <file>`: this player's Ed25519 key (a PKCS#8 PEM file, made there when there is none); the data folder's
    /// unless given.
    pub key: Option<PathBuf>,
    /// `--feed <trace match>`: the local player's inputs are that trace's of the same slot, tick by tick, instead of
    /// the devices' (the end-to-end check's scripted input).
    pub feed: Option<String>,
    /// `--net-cut`, `--net-loss`, `--net-delay`, `--net-jitter`: the faults of this side's relay link (dev).
    pub faults: NetFaults,
}

/// The port's own channel 0 messages between the two players before and after the match.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Say {
    /// The guest to the host until the set-up comes.
    Hi,
    /// The host's: the World's seed, the guest's slot, each slot's id, the stage's key (the port's own field: a
    /// set-up without one is the trace stage's).
    Setup {
        seed: i32,
        slot: usize,
        peers: Vec<String>,
        #[serde(default = "default_stage")]
        stage: String,
    },
    Ping {
        at: f64,
    },
    Pong {
        at: f64,
    },
    /// The guest's level is loaded.
    Loaded,
    /// The host's go: tick 0 is `in` ms from now, on the referee `referee`.
    Go {
        #[serde(rename = "in")]
        r#in: f64,
        referee: String,
    },
    /// The guest's match is over (the host's referee may go).
    Done,
    /// This player's match stopped early (it quit, lost the relay or its referee, or went out of sync): the other's
    /// stops too.
    Left {
        why: String,
    },
}

/// Where the online match is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    /// Loaded, waiting for the go (the host: for the guest's load).
    Load,
    /// The go came: the first frame at this time (ms).
    Gate(f64),
    Play,
    Over,
}

/// A trace's inputs fed as the local player's (`--feed`).
struct Feed {
    trace: Trace,
    book: Book,
    next: usize,
}

/// What one online tick gave the loop (waited / skipped / caught).
#[derive(Debug, Default)]
pub struct OnlineTick {
    /// The events to present now (a waited tick's carried to the next that runs).
    pub events: Option<Vec<Event>>,
    pub waited: bool,
    pub skipped: bool,
    pub caught: bool,
}

/// An online match's tick source: its session, its link and its clock.
pub struct DevOnline {
    client: RelayClient,
    epoch: Instant,
    host: bool,
    lobby: String,
    /// This player's slot, each slot's id.
    slot: usize,
    peers: Vec<String>,
    seed: i32,
    /// The match's stage key (the set-up's; the World's once the match loads, `set_stage_key`).
    stage_key: String,
    feed_name: Option<String>,
    feed: Option<Feed>,
    stage: Stage,
    guest_loaded: bool,
    loaded_sent: bool,
    done_heard: bool,
    last_ping: f64,
    pings: PingWindow,
    session: Option<Session>,
    link: Option<Box<RelayMatchLink>>,
    /// The state hashes' frames by confirmed tick, the newest last.
    hashed: Rc<RefCell<VecDeque<(u32, mvs_trace::Frame)>>>,
    /// A waited tick's events, shown with the next tick that runs.
    carry: Vec<Event>,
    /// Why the match stopped early.
    note: Option<String>,
    /// The desync is reported (once).
    reported: bool,
    /// The relay connection has ended.
    gone: bool,
    /// Why this player's match stopped early (kept; `note` is taken by the loop).
    stopped: Option<String>,
    /// The other player's end is known (its Left, or its link closed): nothing more is said to it.
    other_gone: bool,
    /// The match reached its own end (its results).
    ended: bool,
    /// The start gate's time limit (ms).
    load_until: f64,
}

impl std::fmt::Debug for DevOnline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevOnline").field("slot", &self.slot).field("stage", &self.stage).finish_non_exhaustive()
    }
}

fn now_of(epoch: Instant) -> f64 {
    epoch.elapsed().as_secs_f64() * 1000.0
}

fn say(c: &RelayClient, to: &str, m: &Say) {
    c.send(to, CHANNEL_MESSAGES, &serde_json::to_vec(m).expect("a message serialises"));
}

fn default_stage() -> String {
    mvs_sim_trace::TRACE_STAGE.to_owned()
}

fn heard(e: &RelayEvent) -> Option<(&str, Say)> {
    match e {
        RelayEvent::Packet { from, channel: CHANNEL_MESSAGES, bytes } => {
            serde_json::from_slice(bytes).ok().map(|m| (from.as_str(), m))
        }
        _ => None,
    }
}

fn net_err(e: impl std::fmt::Display) -> Error {
    Error::Match(format!("online: {e}"))
}

/// The online match's start: this player signed in to the relay, the lobby made or joined and the set-up agreed. The
/// match to load, its tick source in its kind.
/// `stage` is the host's stage key; a guest takes the host's.
pub fn connect(o: &OnlineOptions, data: &Path, seed: i32, stage: &str) -> Result<MatchStart> {
    let epoch = Instant::now();
    let key_file = o.key.clone().unwrap_or_else(|| data.join("relay.key"));
    let key = RelayKey::load(&key_file).map_err(net_err)?;
    let app = format!("mvs {}", mvs_release::VERSION);
    let mut c = ClientOptions::new(&app, epoch);
    c.faults = o.faults;
    let client = RelayClient::connect(&o.relay, &key, &c).map_err(net_err)?;
    eprintln!("mvs: online as {} on {}", client.me(), o.relay);
    let until = now_of(epoch) + MEET_MS;
    let gone = |e: &RelayEvent| match e {
        RelayEvent::SignedOut(why) => Some(net_err(why)),
        _ => None,
    };
    let (lobby, slot, peers, seed, stage) = if o.host {
        let lobby = client.create_lobby(BUCKET, 2).map_err(net_err)?;
        // (the line the guest's --join is read from)
        eprintln!("mvs: online lobby {lobby}");
        loop {
            if now_of(epoch) > until {
                return Err(net_err("no one joined"));
            }
            let Some(e) = client.poll() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            if let Some(err) = gone(&e) {
                return Err(err);
            }
            if let Some((from, Say::Hi)) = heard(&e) {
                let peers = vec![client.me().to_owned(), from.to_owned()];
                say(&client, from, &Say::Setup { seed, slot: 1, peers: peers.clone(), stage: stage.to_owned() });
                break (lobby, 0, peers, seed, stage.to_owned());
            }
        }
    } else {
        let lobby = o.join.clone().unwrap_or_default();
        let joined = client.join_lobby(&lobby).map_err(net_err)?;
        let mut hi_at = f64::NEG_INFINITY;
        loop {
            let now = now_of(epoch);
            if now > until {
                return Err(net_err("the host sent no set-up"));
            }
            if now - hi_at >= HI_EVERY_MS {
                hi_at = now;
                say(&client, &joined.owner, &Say::Hi);
            }
            let Some(e) = client.poll() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            if let Some(err) = gone(&e) {
                return Err(err);
            }
            if let Some((from, Say::Setup { seed, slot, peers, stage })) = heard(&e)
                && from == joined.owner
            {
                // (two players, this one in its slot and the host in the other)
                if peers.len() != 2 || slot > 1 || peers[slot] != client.me() || peers[1 - slot] != joined.owner {
                    return Err(net_err(format!("a bad set-up from the host: slot {slot} of {peers:?}")));
                }
                break (lobby, slot, peers, seed, stage);
            }
        }
    };
    eprintln!("mvs: online slot {slot} of {}, seed {seed}, stage {stage}", peers.len());
    let online = DevOnline {
        client,
        epoch,
        host: o.host,
        lobby,
        slot,
        peers,
        seed,
        stage_key: stage.clone(),
        feed_name: o.feed.clone(),
        feed: None,
        stage: Stage::Load,
        guest_loaded: false,
        loaded_sent: false,
        done_heard: false,
        last_ping: f64::NEG_INFINITY,
        pings: PingWindow::default(),
        session: None,
        link: None,
        hashed: Rc::new(RefCell::new(VecDeque::new())),
        carry: Vec::new(),
        note: None,
        reported: false,
        gone: false,
        stopped: None,
        other_gone: false,
        ended: false,
        load_until: f64::INFINITY,
    };
    let mut start = MatchStart::new(
        default_setup(),
        MatchKind::Online { net: Some(Box::new(super::Online::Dev(Box::new(online)))) },
        seed,
    );
    start.stage = stage;
    start.me = slot;
    Ok(start)
}

impl DevOnline {
    fn now(&self) -> f64 {
        now_of(self.epoch)
    }

    /// The stage the match loaded (the World's `data.stage_key`), for the desync report's header.
    pub fn set_stage_key(&mut self, key: &str) {
        key.clone_into(&mut self.stage_key);
    }

    /// The other player's id (the set-up has two, checked when it came).
    fn other(&self) -> &str {
        &self.peers[1 - self.slot]
    }

    /// The match reached its own end (its results): this player's leaving from here on is no early stop.
    pub fn ended(&mut self) {
        self.ended = true;
    }

    /// Why the match stopped early, once.
    pub fn note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// The online start gate, each frame from the match's load to its first tick: the
    /// guest says it is loaded and pings the host; the host, loaded with the guest, has the referee open the match with
    /// tick 0 START_AHEAD_MS on and sends its go; each side starts START_AHEAD_MS less its one-way time (half its
    /// smallest ping; none measured, or one over twice the wait: at once) after the go. True once the match runs: its
    /// session made on `world`.
    pub fn gate(&mut self, world: &World) -> Result<bool> {
        let now = self.now();
        if !self.loaded_sent {
            self.loaded_sent = true;
            self.load_until = now + LOAD_MS;
            if !self.host {
                say(&self.client, self.other(), &Say::Loaded);
            }
        }
        if !self.host && self.stage == Stage::Load && now - self.last_ping >= PING_EVERY_MS {
            self.last_ping = now;
            say(&self.client, self.other(), &Say::Ping { at: now });
        }
        self.messages();
        if self.host && self.stage == Stage::Load && self.guest_loaded {
            let referee = self.open();
            say(&self.client, self.other(), &Say::Go { r#in: START_AHEAD_MS, referee });
            // (the host's way to its own core is no way at all: it waits the whole of it)
            self.stage = Stage::Gate(now + START_AHEAD_MS);
        }
        if matches!(self.stage, Stage::Load | Stage::Gate(_)) && now > self.load_until {
            self.stop("the match did not start in time".to_owned());
        }
        match self.stage {
            Stage::Gate(at) if now >= at && self.stopped.is_none() => {
                self.play(world)?;
                Ok(true)
            }
            Stage::Play => Ok(true),
            _ => Ok(false),
        }
    }

    /// The host's referee: this net thread's (tick 0 START_AHEAD_MS from now; a relay referees a relay-hosted core's
    /// matches only). Its id.
    fn open(&mut self) -> String {
        let humans: Vec<(usize, String)> = self.peers.iter().cloned().enumerate().collect();
        // (every slot is a human's)
        let link = self.client.host_match(self.slots(), humans, self.now() + START_AHEAD_MS);
        self.link = Some(Box::new(link));
        self.client.me().to_owned()
    }

    /// The match's slots, the referee's and the session's: a human each (the set-up's peers).
    fn slots(&self) -> usize {
        self.peers.len()
    }

    /// The match's session at its tick 0, its desync report kept.
    fn play(&mut self, world: &World) -> Result<()> {
        if world.players.len() != self.slots() {
            return Err(net_err(format!("a World of {} players for {} slots", world.players.len(), self.slots())));
        }
        let slots = (0..self.slots()).map(|i| if i == self.slot { SlotKind::Local } else { SlotKind::Remote });
        let mut session = Session::new(world, SessionConfig::new(slots.collect()), self.now())
            .map_err(|e| net_err(format!("the session: {e}")))?;
        let book = Book::new(&world.data);
        let hashed = self.hashed.clone();
        // (each state hash's World written as a trace frame: the one another peer disagrees with is the report)
        session.on_checksum(Box::new(move |ticks, _hash, w: &World| {
            let mut h = hashed.borrow_mut();
            if h.len() >= KEEP_FRAMES {
                h.pop_front();
            }
            h.push_back((ticks, mvs_sim_trace::frame_of(w, &book, &[], &[]).frame));
        }));
        self.session = Some(session);
        if let Some(name) = &self.feed_name {
            let path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
            let trace = Trace::read(&path).map_err(|e| net_err(format!("{}: {e}", path.display())))?;
            self.feed = Some(Feed { trace, book: Book::new(&world.data), next: 0 });
        }
        self.stage = Stage::Play;
        eprintln!("mvs: online match on");
        Ok(())
    }

    /// The relay's news: the other player's messages, a silent human, the connection's end.
    fn messages(&mut self) {
        while let Some(e) = self.client.poll() {
            match &e {
                RelayEvent::SignedOut(why) => {
                    self.gone = true;
                    self.stop(format!("the connection to the relay ended: {why}"));
                }
                RelayEvent::Slot { slot, news: SlotNews::Gone, .. } => self.stop(format!("player {slot} went silent")),
                // Deviation: a leave stops the match when its news comes, not once the confirmed line reaches the end
                // of the leaver's stream: the two players may stop at different ticks.
                RelayEvent::PeerClosed(p) if p == self.other() => {
                    self.other_gone = true;
                    self.stop("the other player left".to_owned());
                }
                RelayEvent::Lobby { closed: Some(why), lobby, .. } if *lobby == self.lobby && !self.host => {
                    self.stop(format!("the lobby closed: {why}"));
                }
                _ => {}
            }
            let Some((from, m)) = heard(&e) else { continue };
            if from != self.other() {
                continue;
            }
            let now = self.now();
            match m {
                Say::Ping { at } => say(&self.client, from, &Say::Pong { at }),
                Say::Pong { at } => self.pings.add(now - at),
                Say::Loaded => self.guest_loaded = true,
                Say::Done => self.done_heard = true,
                Say::Left { why } => {
                    self.other_gone = true;
                    self.stop(format!("the other player stopped: {why}"));
                }
                Say::Go { r#in, referee } if !self.host && self.stage == Stage::Load => {
                    self.link = Some(Box::new(self.client.join_match(&referee)));
                    let ping = self.pings.min();
                    let ahead = if r#in > 0.0 && ping >= 0.0 && ping <= 2.0 * r#in { r#in - ping / 2.0 } else { 0.0 };
                    self.stage = Stage::Gate(now + ahead);
                }
                _ => {}
            }
        }
    }

    /// The match stops early: the loop told once (`note`), the other player at the match's end.
    fn stop(&mut self, why: String) {
        // (after the match's own end the results stay: the other player's leaving is its own end)
        if self.stage != Stage::Over && !self.ended && self.stopped.is_none() {
            self.stopped = Some(why.clone());
            self.note = Some(why);
        }
    }

    /// One 60 Hz tick of the match: the referee's packets taken, the local input read, the
    /// session's frame, its packets sent. `device` is the devices' input of the local player.
    pub fn tick(&mut self, w: &mut World, bots: &mut [Option<BtBot>], device: InputState) -> OnlineTick {
        self.messages();
        let now = self.now();
        let (Some(session), Some(link)) = (&mut self.session, &mut self.link) else { return OnlineTick::default() };
        while let Some(p) = link.recv(now) {
            session.receive(&p, now);
        }
        let input = match &mut self.feed {
            Some(f) => {
                let i = f.trace.frame(f.next).ok().and_then(|fr| {
                    let n = fr.inputs.len().max(1);
                    fr.inputs.get(self.slot % n).map(|r| mvs_sim_trace::input_of(r, w, &f.book))
                });
                f.next += 1;
                i.unwrap_or_default()
            }
            None => device,
        };
        let adv = session.advance(w, bots, input, now);
        for p in session.packets() {
            link.send(&p, now);
        }
        let waited = adv.stalled || adv.skipped;
        let mut events = std::mem::take(&mut self.carry);
        events.extend(adv.events);
        let events = if waited {
            // A frame that ran no tick has its events shown with the next one that runs.
            self.carry = events;
            None
        } else {
            Some(events)
        };
        let desync = session.desync();
        if let Some(d) = desync
            && !self.reported
        {
            self.reported = true;
            let path = self.report(d.tick, d.mine, d.theirs);
            eprintln!(
                "mvs: online desync at confirmed tick {} with slot {} (mine {:08x}, theirs {:08x}): {}",
                d.tick, d.slot, d.mine, d.theirs, path
            );
            self.stop("the match went out of sync".to_owned());
        }
        OnlineTick { events, waited, skipped: adv.skipped, caught: adv.caught }
    }

    /// The desync's report: the confirmed frame of the disagreed hash as a one-frame trace (`trace-diff` against the
    /// other peer's), written beside the working folder; its path, or why there is none.
    fn report(&self, tick: u32, mine: u32, theirs: u32) -> String {
        let hashed = self.hashed.borrow();
        let Some((_, frame)) = hashed.iter().rev().find(|(t, _)| *t == tick) else {
            return format!("no frame kept of tick {tick}");
        };
        let mut out = TraceWriter::new();
        out.push(frame);
        let header = Header {
            format: 0,
            name: format!("desync-slot{}-{tick}", self.slot),
            spec: serde_json::json!({ "online": true, "tick": tick, "mine": mine, "theirs": theirs }),
            setup: serde_json::Value::Null,
            world_seed: f64::from(self.seed),
            stage: Some(self.stage_key.clone()),
            store: serde_json::Value::Null,
            source: serde_json::json!({ "writer": "mvs online" }),
            ticks: 0,
            stop: "desync".to_owned(),
            hashes: Vec::new(),
            final_hash: None,
            calls: Vec::new(),
            schema: Vec::new(),
        };
        let path = PathBuf::from(format!("desync-slot{}-{tick}.trace", self.slot));
        match std::fs::write(&path, out.finish(header)) {
            Ok(()) => std::fs::canonicalize(&path).unwrap_or(path).display().to_string(),
            Err(e) => format!("{}: {e}", path.display()),
        }
    }

    /// (the port's own) The ping line's number: the live round trip to the referee (the host's game), ms.
    pub fn ping(&self) -> Option<f64> {
        self.session.as_ref().and_then(Session::rtt)
    }

    /// The match is over for this player (its results, an early stop, the player's quit or the window's close). At its
    /// own end the guest tells the host; the host's referee stays for the guest's end (the guest's confirmed line still
    /// needs its rulings), up to DONE_WAIT_MS. An early stop tells the other player and waits for nothing. The waiting,
    /// the referee's close and the lobby's leave run on a thread of their own (the window goes on); [`settle`] joins
    /// it at the program's end.
    pub fn finish(mut self) {
        if let Some(s) = &self.session {
            let st = s.stats();
            eprintln!(
                "mvs: online end at confirmed tick {}: {} rollbacks ({} ticks again, deepest {}), {} stalls, {} skips, {} \
                 catch-ups, substituted {:?}, desync {}",
                s.confirmed(),
                st.rollbacks,
                st.resimulated,
                st.deepest,
                st.stalls,
                st.skips,
                st.catchups,
                st.substituted,
                if s.desync().is_some() { "yes" } else { "none" }
            );
        }
        self.messages();
        let early = match &self.stopped {
            Some(why) => Some(why.clone()),
            None if self.ended && self.session.is_some() => None,
            None if self.session.is_some() => Some("the player left the match".to_owned()),
            None => Some("the player left before the match".to_owned()),
        };
        self.stage = Stage::Over;
        if !self.gone && !self.other_gone {
            match &early {
                Some(why) => say(&self.client, self.other(), &Say::Left { why: why.clone() }),
                None if !self.host => say(&self.client, self.other(), &Say::Done),
                None => {}
            }
        }
        let wait = self.host && early.is_none() && !self.done_heard && !self.gone && !self.other_gone;
        let other = self.other().to_owned();
        let end = Ending { client: self.client, epoch: self.epoch, host: self.host, lobby: self.lobby, other, wait };
        match std::thread::Builder::new().name("mvs-online-end".to_owned()).spawn(move || end.run()) {
            Ok(t) => ENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(t),
            Err(e) => eprintln!("mvs: online: the match's end: {e}"),
        }
    }
}

/// The online matches' ends still under way (`DevOnline::finish`).
static ENDING: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// The online matches' ends waited for (the program's end, its window closed).
pub fn settle() {
    let ends = std::mem::take(&mut *ENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    for t in ends {
        let _ = t.join();
    }
}

/// A match's end off the game's thread: the host's wait for the guest's, the referee closed, the lobby left.
struct Ending {
    client: RelayClient,
    epoch: Instant,
    host: bool,
    lobby: String,
    other: String,
    wait: bool,
}

impl Ending {
    fn run(self) {
        let until = now_of(self.epoch) + DONE_WAIT_MS;
        // (its own slot goes silent meanwhile: it ticks no more)
        while self.wait && now_of(self.epoch) < until {
            let Some(e) = self.client.poll() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            let over = match &e {
                RelayEvent::SignedOut(_) => true,
                RelayEvent::PeerClosed(p) => *p == self.other,
                _ => heard(&e).is_some_and(|(from, m)| from == self.other && matches!(m, Say::Done | Say::Left { .. })),
            };
            if over {
                break;
            }
        }
        if self.host {
            self.client.end_match();
        }
        let _ = self.client.leave_lobby(&self.lobby);
        self.client.shutdown();
    }
}
