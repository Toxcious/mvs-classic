//! What the rollback checks share: the game data, the set-ups, the mashers, and the online
//! session over the in-memory network with its referee.
#![allow(dead_code, reason = "each test binary uses a part")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use mvs_data::GameData;
use mvs_net::link::{MatchLink, SimNet, Way, Ways};
use mvs_net::session::{CATCHUP, CHECKSUM_EVERY, HOLD_TICKS, MAX_JUMP, Session, SessionConfig, SlotKind, wire_hash};
use mvs_protocol::match_wire::{INPUT_DELAY, MSG_RULED, NO_END, RESUME_MS, SILENT_MS, TICK_MS, packet_fits, runs};
use mvs_protocol::referee::{GONE_TICKS, SlotNews};
use mvs_sim::ai::bt::BtBot;
use mvs_sim::input::{NO_INPUT, held};
use mvs_sim::setup::MatchSetup;
use mvs_sim::{Event, InputState, World};

/// A peer back after its way was lost is level when its frame is at the referee's clock and its confirmed line no
/// more than this many ticks behind it (its way there and back and the input delay).
const LEVEL_TICKS: f64 = 16.0;
/// ... within this many ticks of its way coming back.
const LEVEL_AFTER: f64 = 60.0;

/// The characters the session checks play.
const CHARS: &[&str] = &["shaggy", "irongiant", "superman", "velma"];

/// The dev store, `None` with a note when there is none (the check passes).
pub fn store() -> Option<mvs_store::Store> {
    let dir = mvs_store::dev_store_dir();
    let s = mvs_store::Store::open(&dir).ok();
    if s.is_none() {
        eprintln!("note: no store at {}; nothing checked", dir.display());
    }
    s
}

/// The game data of [`CHARS`] on the trace stage, loaded once per test binary.
pub fn data() -> Option<Arc<GameData>> {
    static DATA: OnceLock<Option<Arc<GameData>>> = OnceLock::new();
    DATA.get_or_init(|| {
        let s = store()?;
        Some(Arc::new(GameData::load(&s, CHARS, mvs_sim_trace::TRACE_STAGE).expect("the gamedata kind loads")))
    })
    .clone()
}

/// The beta's Hard bot level.
pub const HARD: i32 = 3;

/// `versus(n, bots)`: n players; with a bot level every second one is a bot of that level (two bots alone in a 1v1
/// never approach each other).
pub fn versus(n: usize, bots: Option<i32>) -> serde_json::Value {
    let players: Vec<_> = (0..n)
        .map(|i| serde_json::json!({ "bot": bots.is_some() && i % 2 == 1, "difficulty": bots.unwrap_or(HARD) }))
        .collect();
    serde_json::json!({
        "teamStyle": if n == 2 { 0 } else { 1 },
        "players": players,
        "rules": { "knockoutsToWin": 99, "duration": 420 },
        "flow": true,
    })
}

pub fn setup(v: &serde_json::Value) -> MatchSetup {
    mvs_sim_trace::setup_of(v).expect("a set-up")
}

/// A World of `setup` (seed 9), with the bot trees when a player is a bot.
pub fn world(data: &Arc<GameData>, setup: &MatchSetup) -> World {
    let mut w = World::new(data.clone(), 9, setup).expect("binds");
    if setup.players.iter().any(|p| p.bot) {
        w.bot_trees = Some(Arc::new(data.bot_trees.clone()));
    }
    w
}

/// The match bots (seed 300 + slot, the set-up's level), one slot per player.
pub fn bots(w: &World) -> Vec<Option<BtBot>> {
    w.setup
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let trees = w.bot_trees.as_ref()?;
            #[allow(clippy::cast_possible_truncation, reason = "a level 0..4")]
            let level = p.difficulty.map_or(HARD, |d| d as i32);
            p.bot.then(|| BtBot::match_bot(300 + i32::try_from(i).expect("a slot"), trees, level))
        })
        .collect()
}

/// `bots` with an Expert bot's decisions on the tests' workers (two threads, shared by every test of the binary):
/// the peers' bots, against the reference run's inline ones. `MVS_NET_INLINE=1` keeps them inline (a cost run's).
pub fn bots_on_workers(w: &World) -> Vec<Option<BtBot>> {
    static WORKERS: OnceLock<Arc<mvs_sim::ai::expert::Workers>> = OnceLock::new();
    let mut b = bots(w);
    if std::env::var_os("MVS_NET_INLINE").is_some() {
        return b;
    }
    let workers = WORKERS.get_or_init(|| Arc::new(mvs_sim::ai::expert::Workers::new(2)));
    for bot in b.iter_mut().flatten() {
        bot.set_workers(Some(workers));
    }
    b
}

pub use mvs_sim_trace::inputs::Masher;

/// A peer's fault over ticks `from` .. `to`.
#[derive(Clone, Copy, Debug)]
pub struct Fault {
    pub peer: usize,
    pub from: f64,
    pub to: f64,
}

/// A peer's change at a tick.
#[derive(Clone, Copy, Debug)]
pub struct At {
    pub peer: usize,
    pub at: f64,
}

/// The oracle's rollback `Net`: lat, each peer's one-way time to the referee in ticks; jitter: added at random per
/// packet (the in-memory network lets packets overtake each other); loss: the share of packets lost for good on the
/// way, both ways; start: a peer's first frame; pace: ticks of real time per frame of a peer; steps: a peer's way
/// changes at a tick; cut: a peer's way stands still (its packets, both ways, arrive when it is over); hold: a peer's
/// game stands still (no frame, nothing sent or taken); drop: a peer's way is lost (every packet, both ways, lost for
/// good); leave: its owner calls Referee::left; mute: every packet of a peer is lost from a tick on; deaf: every packet
/// to it; go: the start gate in ticks; fine: the peers nothing is wrong with; twin: a peer whose session runs twice,
/// keeping a predicted line that held and going back every frame; pump_each: the referee pumped after every packet
/// it takes (the relay's core and an app's referee), not on its owner's beat alone. A slot the referee finds gone ends the match: its
/// owner takes it out, and every other peer stops once its confirmed line reaches the end of that slot's stream.
#[derive(Clone, Debug, Default)]
pub struct Net {
    pub lat: Vec<f64>,
    pub jitter: f64,
    pub loss: f64,
    pub start: Vec<f64>,
    pub pace: Vec<f64>,
    pub steps: Vec<(usize, f64, f64)>,
    pub cut: Vec<Fault>,
    pub hold: Vec<Fault>,
    pub drop: Vec<Fault>,
    pub leave: Vec<At>,
    pub mute: Vec<At>,
    pub deaf: Vec<At>,
    pub go: f64,
    pub fine: Vec<usize>,
    pub twin: Option<usize>,
    pub pump_each: bool,
}

struct Peer {
    s: Session,
    w: World,
    bots: Vec<Option<BtBot>>,
    slot: usize,
    m: Masher,
    got: Rc<RefCell<BTreeMap<u32, u32>>>,
    next: f64,
    first: f64,
    pace: f64,
    stopped: bool,
    behind: i64,
    /// Per standstill: its end, the ticks after it at which the peer was at the clock again (-1: not yet).
    back: Vec<(f64, f64)>,
    /// Per lost or cut way: its end, the ticks after it at which the peer was level again (at the clock, its confirmed
    /// line near it; -1: not yet).
    level: Vec<(f64, f64)>,
    /// The most ticks its shown line was ahead of its confirmed one after a frame, and its session's limit; the
    /// snapshot's lag its session allows (a rollback runs limit + keep ticks at the most).
    most_predicted: u32,
    limit: u32,
    keep: u32,
    /// When it stopped as the match ended (a gone slot).
    ended: Option<f64>,
    intro: BTreeMap<(bool, i32), u32>,
    /// Per frame: the wall time of its advance (ms), the ticks it confirmed and the World steps it ran.
    frames: Vec<(f64, u32, u64)>,
}

/// What a session check found: `Err` with the first thing wrong, else its summary.
pub type Outcome = Result<String, String>;

/// `sessionTest`: the peers of `setup` online over the in-memory network for `ticks` confirmed ticks (the oracle's
/// rollback check): every confirmed hash equal to a reference World stepped on the ruled streams, each fighter's intro
/// events shown once, the fine peers never waiting nor leaving a frame out nor more than 3 ticks behind, every
/// packet fitting, the twin showing and sending the same, the away, gone and leaving peers as the case says.
#[allow(clippy::too_many_lines, reason = "the oracle check's one function, in its order")]
#[allow(clippy::cast_possible_truncation, reason = "ticks of a short match")]
pub fn session_test(setup: &MatchSetup, ticks: u32, net: &Net) -> Option<Outcome> {
    let data = data()?;
    let slots = setup.players.len();
    let humans: Vec<usize> = setup.players.iter().enumerate().filter(|(_, p)| !p.bot).map(|(i, _)| i).collect();
    let ms = |t: f64| t * TICK_MS;
    let way = |lat: f64| Way { latency: lat * TICK_MS, jitter: net.jitter * TICK_MS, loss: net.loss, cut: false };
    let mut lat = net.lat.clone();
    let mut sim = SimNet::new(slots, &humans, Ways { up: way(0.0), down: way(0.0) }, 0.0, 77);
    // the ways of each peer: its latency, cut while a cut or a mute / deaf case says so
    let ways_at = |p: usize, t: f64, lat: f64| {
        let cut = net.cut.iter().any(|c| c.peer == p && t >= c.from && t < c.to);
        let mute = net.mute.iter().any(|x| x.peer == p && t >= x.at);
        let deaf = net.deaf.iter().any(|x| x.peer == p && t >= x.at);
        let lost = net.drop.iter().any(|c| c.peer == p && t >= c.from && t < c.to);
        let w = if lost { Way { loss: 1.0, ..way(lat) } } else { way(lat) };
        Ways { up: Way { cut: cut || mute, ..w }, down: Way { cut: cut || deaf, ..w } }
    };
    let mut shown_ways: Vec<Ways> = humans.iter().enumerate().map(|(p, _)| ways_at(p, -net.go, lat[p])).collect();
    for (p, &h) in humans.iter().enumerate() {
        sim.set_ways(h, shown_ways[p]);
    }
    sim.log_sent();
    let mut bad = String::new();
    let fail = |bad: &mut String, m: String| {
        if bad.is_empty() {
            *bad = m;
        }
    };
    let slots_of = |slot: usize| -> Vec<SlotKind> {
        (0..slots)
            .map(|i| {
                if setup.players.get(i).is_some_and(|p| p.bot) {
                    SlotKind::Bot
                } else if i == slot {
                    SlotKind::Local
                } else {
                    SlotKind::Remote
                }
            })
            .collect()
    };
    let mut now = -net.go;
    // the twin: its World, bots and session going back every frame
    let mut twin: Option<(Session, World, Vec<Option<BtBot>>)> = net.twin.map(|p| {
        let w = world(&data, setup);
        let cfg = SessionConfig { keep_behind: 0, ..SessionConfig::new(slots_of(humans[p])) };
        let s = Session::new(&w, cfg, ms(now)).expect("a session");
        let b = bots_on_workers(&w);
        (s, w, b)
    });
    let mut twin_frames = 0;
    let mut peers: Vec<Peer> = humans
        .iter()
        .enumerate()
        .map(|(p, &slot)| {
            let w = world(&data, setup);
            let cfg = SessionConfig::new(slots_of(slot));
            let (limit, keep) = (cfg.max_prediction, cfg.keep_behind);
            let mut s = Session::new(&w, cfg, ms(now)).expect("a session");
            let got = Rc::new(RefCell::new(BTreeMap::new()));
            let g = got.clone();
            s.on_checksum(Box::new(move |t, v, _| {
                g.borrow_mut().insert(t, v);
            }));
            // the start gate: the go comes over the peer's way, then the wait less its one-way time
            let mut first = net.start.get(p).copied().unwrap_or(0.0);
            if net.go > 0.0 {
                let (go_ms, ping) = (ms(net.go), 2.0 * ms(lat[p]));
                let wait = if ping <= 2.0 * go_ms { go_ms - ping / 2.0 } else { 0.0 };
                first += now + lat[p] + wait / TICK_MS;
            }
            let bots = bots_on_workers(&w);
            Peer {
                s,
                w,
                bots,
                slot,
                m: Masher::new(40 + i32::try_from(slot).expect("a slot")),
                got,
                next: first,
                first,
                pace: net.pace.get(p).copied().unwrap_or(1.0),
                stopped: false,
                behind: 0,
                back: net.hold.iter().filter(|h| h.peer == p).map(|h| (h.to, -1.0)).collect(),
                level: net.drop.iter().chain(&net.cut).filter(|h| h.peer == p).map(|h| (h.to, -1.0)).collect(),
                most_predicted: 0,
                limit,
                keep,
                ended: None,
                intro: BTreeMap::new(),
                frames: Vec::new(),
            }
        })
        .collect();
    let holding = |p: usize, t: f64| net.hold.iter().any(|h| h.peer == p && t >= h.from && t < h.to);
    // the ruled streams by slot and tick (None: a substitution), and where a leaver's ends
    let mut ruled: Vec<Vec<Option<Option<InputState>>>> = vec![Vec::new(); slots];
    let mut ends: BTreeMap<usize, u32> = BTreeMap::new();
    let mut out: Vec<bool> = vec![false; humans.len()];
    let mut news_seen = 0;
    let mut news_at: Vec<(usize, SlotNews, f64)> = Vec::new();
    // (the slot found gone, which ends the match)
    let mut gone: Option<usize> = None;
    let mut leaves = net.leave.clone();
    let mut steps = net.steps.clone();
    let (mut packets, mut most, mut most_kept, mut shown) = (0usize, 0usize, 0usize, 0usize);
    // (the bytes the referee sent each slot from tick 0, and each slot's peer sent it; the ticks each frame showed
    // past its confirmed line, summed, and the frames)
    let (mut down, mut up) = (vec![0usize; slots], vec![0usize; slots]);
    let (mut ahead, mut frames) = (0u64, 0u64);
    let step = 1.0 / 16.0;
    let mut pump_at = -net.go;
    let tickf = f64::from(ticks);
    // (a cost run's: MVS_NET_THROTTLE=k stretches each advance k times by a spin, as the game's --throttle;
    // MVS_NET_REAL_PACE=1 runs the simulated clock at the real one, so the Expert's workers have a frame's time)
    let throttle: f64 = std::env::var("MVS_NET_THROTTLE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let real_pace = std::env::var_os("MVS_NET_REAL_PACE").is_some();
    let (wall0, now0) = (std::time::Instant::now(), now);
    while now < tickf * 4.0 && peers.iter().any(|p| !p.stopped && p.s.confirmed() < ticks) {
        for x in &mut steps {
            if x.1 >= 0.0 && now >= x.1 {
                lat[x.0] = x.2;
                x.1 = -1.0;
            }
        }
        for (p, &h) in humans.iter().enumerate() {
            let w = ways_at(p, now, lat[p]);
            if w != shown_ways[p] {
                sim.set_ways(h, w);
                shown_ways[p] = w;
            }
        }
        let take_out = |sim: &mut SimNet, out: &mut Vec<bool>, p: usize| {
            if !out[p] {
                out[p] = true;
                sim.left(humans[p], ms(now));
            }
        };
        for x in &mut leaves {
            if x.at >= 0.0 && now >= x.at {
                x.at = -1.0;
                take_out(&mut sim, &mut out, x.peer);
            }
        }
        if net.pump_each {
            sim.deliver_pumped(ms(now));
        } else {
            sim.deliver(ms(now));
        }
        // (the owner's beat: every tick)
        if now >= pump_at {
            sim.pump(ms(now));
            for &h in &humans {
                most_kept = most_kept.max(sim.referee().kept(h));
            }
            pump_at += 1.0;
        }
        for (slot, bytes) in sim.take_sent() {
            packets += 1;
            most = most.max(bytes.len());
            if now >= 0.0 {
                down[slot] += bytes.len();
            }
            if !packet_fits(&bytes, slots) {
                fail(&mut bad, format!("a packet of type {} and {} bytes does not fit", bytes[0], bytes.len()));
            }
            if bytes[0] != MSG_RULED {
                continue;
            }
            for run in runs(&bytes) {
                let s = usize::from(run.head.slot);
                if run.head.end != NO_END {
                    if ends.get(&s).is_some_and(|&e| e != run.head.end) {
                        fail(&mut bad, format!("slot {s}: two ends, {} and {}", ends[&s], run.head.end));
                    }
                    ends.insert(s, run.head.end);
                }
                let mut at = 0;
                for k in 0..u32::from(run.head.count) {
                    let sub = (run.head.mask >> k) & 1 == 1;
                    let x = if sub {
                        None
                    } else {
                        at += 1;
                        Some(mvs_net::codec::decode(&run.inputs[at - 1]))
                    };
                    let t = (run.head.first + k) as usize;
                    let l = &mut ruled[s];
                    match l.get(t).copied().flatten() {
                        None => {
                            if t > (INPUT_DELAY as usize).max(l.len()) {
                                fail(&mut bad, format!("slot {s}: tick {t} was ruled before tick {}", l.len()));
                            }
                            if l.len() <= t {
                                l.resize(t + 1, None);
                            }
                            l[t] = Some(x);
                        }
                        Some(had) if had != x => fail(&mut bad, format!("slot {s}: tick {t} was ruled two ways")),
                        Some(_) => {}
                    }
                }
            }
        }
        while news_seen < sim.news().len() {
            let (s, n) = sim.news()[news_seen];
            news_seen += 1;
            news_at.push((s, n, now));
            if n == SlotNews::Gone
                && let Some(p) = humans.iter().position(|&h| h == s)
            {
                take_out(&mut sim, &mut out, p);
                gone.get_or_insert(s);
            }
        }
        for (i, p) in peers.iter_mut().enumerate() {
            if out[i] {
                p.stopped = true;
            }
            // (the match ends at a gone slot: told it left, a peer stops once its confirmed line is at its stream's
            // end)
            if let Some(g) = gone
                && !p.stopped
                && p.s.end_of(g).is_some_and(|e| p.s.confirmed() >= e)
            {
                p.stopped = true;
                p.ended = Some(now);
            }
            if p.stopped || holding(i, now) {
                continue;
            }
            // what has come for it (a game that stands still takes it when it goes again)
            while let Some(b) = sim.link(p.slot).recv(ms(now)) {
                p.s.receive(&b, ms(now));
                if net.twin == Some(i) {
                    twin.as_mut().expect("the twin").0.receive(&b, ms(now));
                }
            }
            if now < p.next {
                continue;
            }
            // (a game that stood still owes a quarter second of ticks at the most: the loop's limit)
            p.next = p.next.max(now - 15.0) + p.pace;
            let input = p.m.at(p.s.frame() as usize);
            let (t0, c0, s0) = (std::time::Instant::now(), p.s.confirmed(), p.s.stats().steps);
            let res = p.s.advance(&mut p.w, &mut p.bots, input, ms(now));
            p.most_predicted = p.most_predicted.max(p.s.predicted());
            if now >= 0.0 {
                ahead += u64::from(p.s.predicted());
                frames += 1;
            }
            if throttle > 1.0 {
                let until = std::time::Instant::now() + t0.elapsed().mul_f64(throttle - 1.0);
                while std::time::Instant::now() < until {
                    std::hint::spin_loop();
                }
            }
            p.frames.push((t0.elapsed().as_secs_f64() * 1000.0, p.s.confirmed() - c0, p.s.stats().steps - s0));
            let sent: Vec<Vec<u8>> = p.s.packets().collect();
            for b in &sent {
                if usize::from(b[1]) != p.slot {
                    fail(&mut bad, format!("peer {i} sent a packet that says slot {}", b[1]));
                }
                packets += 1;
                most = most.max(b.len());
                if !packet_fits(b, slots) {
                    fail(&mut bad, format!("a packet of type {} and {} bytes does not fit", b[0], b.len()));
                }
                if now >= 0.0 {
                    up[p.slot] += b.len();
                }
                sim.link(p.slot).send(b, ms(now));
            }
            if net.twin == Some(i) {
                let (ts, tw, tb) = twin.as_mut().expect("the twin");
                let r2 = ts.advance(tw, tb, input, ms(now));
                twin_frames += 1;
                if r2 != res {
                    fail(
                        &mut bad,
                        format!(
                            "peer {i}, frame {}: the session going back every frame shows other events",
                            p.s.frame()
                        ),
                    );
                }
                let sent2: Vec<Vec<u8>> = ts.packets().collect();
                if sent2 != sent {
                    fail(
                        &mut bad,
                        format!(
                            "peer {i}, frame {}: the session going back every frame sends other packets",
                            p.s.frame()
                        ),
                    );
                }
            }
            shown += res.events.len();
            for e in &res.events {
                match e {
                    Event::IntroFx { fighter } => *p.intro.entry((false, *fighter)).or_default() += 1,
                    Event::IntroReveal { fighter } => *p.intro.entry((true, *fighter)).or_default() += 1,
                    _ => {}
                }
            }
            // (a jump to the clock: the frame owes no more ticks, the loop's accumulator starts again)
            if res.caught {
                p.next = now + p.pace;
            }
            if let Some(d) = p.s.desync() {
                fail(&mut bad, format!("desync reported at tick {}", d.tick));
            }
            if now >= p.first + 1.0 {
                p.behind = p.behind.max(now.floor() as i64 - i64::from(p.s.frame()));
            }
            for b in &mut p.back {
                if b.1 < 0.0 && now >= b.0 && f64::from(p.s.frame()) >= now.floor() - 2.0 {
                    b.1 = now - b.0;
                }
            }
            for b in &mut p.level {
                if b.1 < 0.0
                    && now >= b.0
                    && f64::from(p.s.frame()) >= now.floor() - 2.0
                    && f64::from(p.s.confirmed()) >= now.floor() - LEVEL_TICKS
                {
                    b.1 = now - b.0;
                }
            }
        }
        now += step;
        if real_pace {
            let at = wall0 + std::time::Duration::from_secs_f64((now - now0) * TICK_MS / 1000.0);
            std::thread::sleep(at.saturating_duration_since(std::time::Instant::now()));
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "byte counts of a short match")]
    let rate = |b: usize| b as f64 / (now.max(1.0) * TICK_MS / 1000.0);
    #[allow(clippy::cast_precision_loss, reason = "counts of a short match")]
    let mean_ahead = ahead as f64 / frames.max(1) as f64;
    let bytes_note = format!(
        "bytes a second from the referee {:.0}, to each peer {:?}, from each peer {:?}; a frame's shown line {mean_ahead:.2} ticks past its confirmed one (mean)",
        rate(down.iter().sum()),
        down.iter().map(|&b| rate(b).round()).collect::<Vec<_>>(),
        up.iter().map(|&b| rate(b).round()).collect::<Vec<_>>()
    );
    // (a match a gone slot ended: checked up to its end)
    let ticks = match gone.and_then(|g| ends.get(&g)) {
        Some(&e) => ticks.min(e),
        None => ticks,
    };
    let tickf = f64::from(ticks);
    // the reference: one World on the ruled streams (the substitution's rule as the session has it)
    let end_of = |h: usize| ends.get(&h).copied().unwrap_or(u32::MAX);
    let stream: Vec<Vec<InputState>> = humans
        .iter()
        .map(|&h| {
            let mut l = vec![NO_INPUT; INPUT_DELAY as usize];
            let mut last: Option<usize> = None;
            for t in INPUT_DELAY as usize..(ticks.min(end_of(h)) as usize) {
                let Some(Some(x)) = ruled[h].get(t).copied() else { break };
                let input = x.unwrap_or_else(|| match last {
                    Some(l0) if t - l0 <= HOLD_TICKS as usize => held(&l[l0]),
                    _ => NO_INPUT,
                });
                if x.is_some() {
                    last = Some(t);
                }
                l.push(input);
            }
            l
        })
        .collect();
    let mut want = BTreeMap::new();
    // (each bot's think of the reference run, ms)
    let mut thinks: Vec<f64> = Vec::new();
    if let Some(k) = humans.iter().enumerate().position(|(k, &h)| stream[k].len() < ticks.min(end_of(h)) as usize) {
        fail(&mut bad, format!("the referee ruled only {} ticks of slot {}", stream[k].len(), humans[k]));
    }
    if bad.is_empty() {
        let mut w = world(&data, setup);
        let mut b = bots(&w);
        for t in 0..ticks {
            let ins: Vec<InputState> = (0..setup.players.len())
                .map(|i| match b[i].as_mut() {
                    Some(bot) => {
                        let me = w.players[i];
                        let t0 = std::time::Instant::now();
                        let input = bot.think(&mut w, me);
                        thinks.push(t0.elapsed().as_secs_f64() * 1000.0);
                        input
                    }
                    None if t >= end_of(i) => NO_INPUT,
                    None => stream[humans.iter().position(|&h| h == i).expect("a human")][t as usize],
                })
                .collect();
            w.step(&ins);
            w.events.clear();
            if (t + 1) % CHECKSUM_EVERY == 0 {
                want.insert(t + 1, wire_hash(mvs_sim::state::hash_of(&w)));
            }
        }
    }
    let mut sums = 0;
    for (i, p) in peers.iter().enumerate() {
        if !p.stopped && p.s.confirmed() < ticks {
            fail(&mut bad, format!("peer {i} confirmed only {} ticks", p.s.confirmed()));
        }
        for (&t, &v) in &want {
            if !bad.is_empty() || t > p.s.confirmed() {
                break;
            }
            sums += 1;
            let got = p.got.borrow().get(&t).copied();
            if got != Some(v) {
                fail(
                    &mut bad,
                    format!("peer {i}: hash after {t} ticks is {got:x?}, the World on the ruled inputs gives {v:x}"),
                );
            }
        }
    }
    // the intro's events, once per fighter on every peer (a wait or a jump over tick 150 included)
    for (i, p) in peers.iter().enumerate() {
        if p.stopped && p.s.confirmed() < 200 {
            continue;
        }
        for reveal in [false, true] {
            for f in 0..setup.players.len() {
                let n = p.intro.get(&(reveal, i32::try_from(f).expect("a fighter"))).copied().unwrap_or(0);
                if n != 1 {
                    let kind = if reveal { "introReveal" } else { "introFx" };
                    fail(&mut bad, format!("peer {i} showed {kind} of fighter {f} {n} times"));
                }
            }
        }
    }
    let count = |slot: usize, from: f64, to: f64| {
        let lo = (from.max(f64::from(INPUT_DELAY))) as usize;
        let hi = (to.min(tickf).min(f64::from(end_of(slot)))) as usize;
        (lo..hi).filter(|&t| ruled[slot].get(t) == Some(&Some(None))).count()
    };
    let mut notes: Vec<String> = vec![bytes_note];
    for &i in &net.fine {
        let st = peers[i].s.stats();
        if st.stalls > 0 || st.skips > 0 {
            fail(
                &mut bad,
                format!("peer {i}, nothing wrong with it, waited {} frames and left {} out", st.stalls, st.skips),
            );
        } else if peers[i].behind > 3 {
            fail(
                &mut bad,
                format!("peer {i}, nothing wrong with it, was {} ticks behind the referee's clock", peers[i].behind),
            );
        }
    }
    for h in &net.hold {
        let p = &peers[h.peer];
        let b = p.back.iter().find(|x| x.0 == h.to).expect("the standstill").1;
        let limit = 4.0 + ((h.to - h.from) / f64::from(MAX_JUMP)).floor();
        if h.to < tickf && (b < 0.0 || b > limit) {
            fail(
                &mut bad,
                format!("peer {} was not back at the referee's clock {b} ticks after tick {}", h.peer, h.to),
            );
        } else if h.to - h.from > f64::from(CATCHUP) && p.s.stats().catchups == 0 {
            fail(&mut bad, format!("peer {} did not jump to the referee's clock after its game stood still", h.peer));
        }
        notes.push(format!(
            "peer {} stood still {}..{}: back at the clock {b:.1} ticks after, slot {} {} ticks substituted then",
            h.peer,
            h.from,
            h.to,
            p.slot,
            count(p.slot, h.from, h.to + 10.0)
        ));
    }
    for (i, p) in peers.iter().enumerate() {
        if p.most_predicted > p.limit {
            fail(&mut bad, format!("peer {i} predicted {} ticks, past its limit {}", p.most_predicted, p.limit));
        }
        let deepest = p.s.stats().deepest;
        if deepest > p.limit + p.keep {
            fail(&mut bad, format!("peer {i} rolled back {deepest} ticks, past {} + {}", p.limit, p.keep));
        }
    }
    for c in &net.cut {
        let s = humans[c.peer];
        // (a way that stands still longer than the prediction limit: the peer waits at it, and is level once its way
        // goes again)
        let p = &peers[c.peer];
        let level = p.level.iter().find(|x| x.0 == c.to).expect("the cut way").1;
        if c.to - c.from > f64::from(p.limit) + 2.0 * lat[c.peer] + 4.0 {
            if p.most_predicted != p.limit || p.s.stats().stalls == 0 {
                fail(
                    &mut bad,
                    format!(
                        "peer {}, its way still {}..{}: predicted {} of its limit {}, waited {} frames",
                        c.peer,
                        c.from,
                        c.to,
                        p.most_predicted,
                        p.limit,
                        p.s.stats().stalls
                    ),
                );
            }
            if c.to < tickf && !(0.0..=LEVEL_AFTER).contains(&level) {
                fail(&mut bad, format!("peer {} was not level {level} ticks after its way went at {}", c.peer, c.to));
            }
        }
        notes.push(format!(
            "slot {s}'s way stood still {}..{}: {} ticks substituted then, level {level:.1} ticks after, at most {} \
             predicted (limit {})",
            c.from,
            c.to,
            count(s, c.from, c.to + 10.0),
            p.most_predicted,
            p.limit
        ));
    }
    // the referee's news: a mute or deaf peer away once, then gone; a lost way away and back, or away and gone, by
    // its length; nothing of any other slot
    let news_of =
        |s: usize| -> Vec<(SlotNews, f64)> { news_at.iter().filter(|x| x.0 == s).map(|x| (x.1, x.2)).collect() };
    let quiet: Vec<At> = net.mute.iter().chain(&net.deaf).copied().collect();
    for x in &quiet {
        let s = humans[x.peer];
        let got = news_of(s);
        let kinds: Vec<SlotNews> = got.iter().map(|g| g.0).collect();
        if kinds != [SlotNews::Away, SlotNews::Gone] {
            fail(&mut bad, format!("slot {s}, quiet from tick {}: the referee's news {kinds:?}", x.at));
        } else {
            notes.push(format!(
                "slot {s} away {:.0} ticks and gone {:.0} ticks after it went quiet",
                got[0].1 - x.at,
                got[1].1 - x.at
            ));
        }
    }
    for d in &net.drop {
        let s = humans[d.peer];
        let ms = (d.to - d.from) * TICK_MS;
        let want: &[SlotNews] = if ms < SILENT_MS - 500.0 {
            &[]
        } else if ms < RESUME_MS - 500.0 {
            &[SlotNews::Away, SlotNews::Back]
        } else {
            &[SlotNews::Away, SlotNews::Gone]
        };
        let got = news_of(s);
        let kinds: Vec<SlotNews> = got.iter().map(|g| g.0).collect();
        if kinds != want {
            fail(&mut bad, format!("slot {s}, its way lost {}..{}: the referee's news {kinds:?}", d.from, d.to));
            continue;
        }
        let when: Vec<String> = got.iter().map(|g| format!("{:?} at {:.0}", g.0, g.1)).collect();
        if want.last() == Some(&SlotNews::Gone) {
            // (gone RESUME_MS after it was last heard, about when its way was lost)
            let late = got[1].1 - d.from - f64::from(GONE_TICKS);
            if !(-5.0..=10.0).contains(&late) {
                fail(&mut bad, format!("slot {s} was found gone {late:.0} ticks off RESUME_MS after its way was lost"));
            }
            for (i, p) in peers.iter().enumerate() {
                if p.slot != s && p.ended.is_none_or(|e| e - got[1].1 > 60.0) {
                    fail(&mut bad, format!("peer {i}: the match did not end within 60 ticks of slot {s} gone"));
                }
            }
            notes.push(format!(
                "slot {s}'s way lost {}..{}: {}; the match ended on every peer",
                d.from,
                d.to,
                when.join(", ")
            ));
        } else {
            let p = &peers[d.peer];
            let b = p.level.iter().find(|x| x.0 == d.to).expect("the lost way").1;
            if d.to < tickf && !(0.0..=LEVEL_AFTER).contains(&b) {
                fail(&mut bad, format!("peer {} was not level {b} ticks after its way came back at {}", d.peer, d.to));
            }
            notes.push(format!(
                "slot {s}'s way lost {}..{}: {}; level {b:.1} ticks after, {} ticks substituted",
                d.from,
                d.to,
                when.join(", "),
                count(s, d.from, d.to + 10.0)
            ));
        }
    }
    for &(s, n, _) in &news_at {
        if !quiet.iter().any(|x| humans[x.peer] == s) && !net.drop.iter().any(|d| humans[d.peer] == s) {
            fail(&mut bad, format!("the referee told {n:?} of slot {s}, which nothing was wrong with"));
        }
    }
    if gone.is_some() && peers.iter().any(|p| !p.stopped) {
        fail(&mut bad, "a slot was found gone and the match did not end on every peer".to_owned());
    }
    // (the streams kept for an away peer: GONE_TICKS of them and a little)
    if most_kept > GONE_TICKS as usize + 100 {
        fail(&mut bad, format!("the referee kept {most_kept} ticks of a stream"));
    }
    for x in &net.leave {
        if !ends.contains_key(&humans[x.peer]) {
            fail(&mut bad, format!("slot {} left and its stream has no end", humans[x.peer]));
        }
    }
    if !ends.is_empty() {
        notes.push(format!("ends: {ends:?}"));
    }
    if let Some((ts, ..)) = &twin {
        notes.push(format!("the twin the same over {twin_frames} frames ({} rollbacks there)", ts.stats().rollbacks));
    }
    let ge: Vec<Option<u32>> = peers.iter().filter(|p| !p.stopped).map(|p| p.s.game_end()).collect();
    if ge.iter().any(|e| *e != ge[0]) {
        fail(&mut bad, format!("the peers confirmed the match's end at ticks {ge:?}"));
    }
    if let Some(Some(e)) = ge.first() {
        notes.push(format!("GameEnd confirmed at tick {e} on every peer"));
    }
    let st: Vec<_> = peers.iter().map(|p| p.s.stats().clone()).collect();
    let rollbacks: u64 = st.iter().map(|x| x.rollbacks).sum();
    let summary = format!(
        "{ticks} ticks, {} peers, {}, {sums} hashes equal, {rollbacks} rollbacks (deepest {}), waits {:?}, skips {:?}, catch-ups {:?}, substituted {:?}, the most behind the clock {:?}, delays {:?} (missed {:?}), {packets} packets (largest {most} bytes), {shown} events shown; {}",
        peers.len(),
        frame_cost(&peers, &thinks),
        st.iter().map(|x| x.deepest).max().unwrap_or(0),
        st.iter().map(|x| x.stalls).collect::<Vec<_>>(),
        st.iter().map(|x| x.skips).collect::<Vec<_>>(),
        st.iter().map(|x| x.catchups).collect::<Vec<_>>(),
        humans.iter().map(|&h| count(h, 0.0, tickf)).collect::<Vec<_>>(),
        peers.iter().map(|p| p.behind).collect::<Vec<_>>(),
        peers.iter().map(|p| p.s.local_delay()).collect::<Vec<_>>(),
        st.iter().map(|x| x.missed).collect::<Vec<_>>(),
        notes.join("; ")
    );
    Some(if bad.is_empty() { Ok(summary) } else { Err(bad) })
}

/// Wall times' mean / p99 / max, in ms.
fn spread(ms: &[f64]) -> String {
    if ms.is_empty() {
        return "none".to_owned();
    }
    let mut ms = ms.to_vec();
    ms.sort_by(f64::total_cmp);
    #[allow(clippy::cast_precision_loss, reason = "a count of frames")]
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    let p99 = ms[(ms.len() * 99 / 100).min(ms.len() - 1)];
    format!("{mean:.3} / {p99:.3} / {:.3} ms (mean / p99 / max)", ms[ms.len() - 1])
}

/// The peers' frames' cost: the wall time of an advance over every frame of every peer, the ticks a frame confirmed
/// (each of them a think of every bot) and the World steps it ran (p99 / max), and the wall time of a bot's think in
/// the reference run.
fn frame_cost(peers: &[Peer], thinks: &[f64]) -> String {
    let ms: Vec<f64> = peers.iter().flat_map(|p| p.frames.iter().map(|f| f.0)).collect();
    let mut confirmed: Vec<u64> = peers.iter().flat_map(|p| p.frames.iter().map(|f| u64::from(f.1))).collect();
    let mut steps: Vec<u64> = peers.iter().flat_map(|p| p.frames.iter().map(|f| f.2)).collect();
    let top = |v: &mut Vec<u64>| {
        v.sort_unstable();
        let p99 = v.get((v.len() * 99 / 100).min(v.len().saturating_sub(1))).copied().unwrap_or(0);
        format!("{p99} / {}", v.last().copied().unwrap_or(0))
    };
    format!(
        "frames {}, ticks confirmed in one {} and steps {} (p99 / max), a bot's think {}",
        spread(&ms),
        top(&mut confirmed),
        top(&mut steps),
        spread(thinks)
    )
}

/// Runs a session check and fails the test on what it found (prints the summary either way).
pub fn check(name: &str, setup: &MatchSetup, ticks: u32, net: &Net) {
    let Some(r) = session_test(setup, ticks, net) else { return };
    match r {
        Ok(s) => println!("ok   {name}: {s}"),
        Err(e) => panic!("FAIL {name}: {e}"),
    }
}
