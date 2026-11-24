//! The game. With `--play`: the title, the menus and their matches on the store `--store` names (the launcher,
//! `mvs-launcher.exe`, starts it so); else one match in a window, from its load to its results.
//!
//! mvs --play --store <dir> [--data <dir>] [--size 1920x1080] [--full-screen] [--adapter <text>] [--no-audio]
//!   [--online-defaults <file>]
//!   (a `--play` run signs in to the backends of the release's online.json, beside the program unless
//!   `--online-defaults` names it, with the player's own online settings over it; with no `--store` it hands over
//!   to the launcher beside it, which plays at once when the game is ready)
//! mvs [--data <dir>] [--store <dir>] [--size 1920x1080] [--full-screen] [--adapter <text>] [--seed <n>] [--no-ui]
//!   [--replay <trace match> [--trace-out <file>]] [--real-pace] [--shot <tick>[,..]] [--out <dir>] [--throttle <rate>]
//!   [--bench <secs>] [--stage <key>] [--stage-layout <file>] [--bots <level>] [--shot-results <secs>] [--press-results <secs>] [--no-audio] [--input-script <jsonl>] [--menu-tape <out>]
//!   [--shot-frames <frame>[,..]] [--live-loads] [--audio-tape <file>]
//!   [--waves endless|100 [--character <key>] [--waves-bot <level>]]   (a Waves run on `--stage`, cast from `--seed`)
//!   [--wheel [--wheel-seconds <n>]]   (with `--play` too: the Chaos Wheel rule in the command line's match, a Lab and Local Play;
//!   with `--relay`, on both players' lines)
//!   [--relay <addr> (--host | --join <lobby>) [--key <file>] [--feed <trace match>]]
//!   [--online-config <file> [--direct-loopback] [--online-open host|join:<code>|queue:<mode>[,..]] [--online-lag <ms>]
//!   [--online-desync-at <tick>] [--feed <trace match>]]
//!   [--queue-fake [--queue-bots <s>] [--queue-start <s>] [--queue-vote-no] [--queue-path <pm.path json>]]
//!   [--fake-lobbies] (dev: the custom games list shows ten made-up lobbies)
//!   [--net-cut <s> [--net-cut-at <s>]] [--net-loss <share>] [--net-delay <ms>] [--net-jitter <ms>]
//!   (dev: faults on this side's relay link, any run that goes online: a cut of <s> seconds from <s> (10 unless
//!   given) after the online match's go, the share of datagrams lost, a delay each way and up to that much more)
//! mvs --import-worker <op>   (an import process: its request on stdin, its events on stdout; the self-test's)
//! mvs --update-now           (check the update feed, download and hand over, with no window)
//! mvs --finish-update <dir> --wait-pid <pid>   (an unpacked release copying itself over <dir>)
//! mvs --version   (prints `mvs <version>` and leaves)
//! mvs --self-test [--data <dir>] [--store <dir>] [--check-out <file>]   (the release's checks, no window:
//!   `mvs_app::selftest`; --check-out writes the self check's reference, the release tool's packing run)
//! mvs --net-test [--data <dir>] [--online-defaults <file>] [--relay-only] [--direct-loopback]   (the launcher's Test connection: its result as JSON,
//!   `mvs_app::launcher::net_test`)
//!
//! mvs --load-bench [--store <dir>] [--adapter <text>]   (the loads timed step by step: `mvs_app::load_bench`)
//! mvs --match-sweep [--store <dir>] [--sweep-ticks <n>] [--sweep-only <key>[,..]]   (every fighter and skin loaded and
//!   run: `mvs_app::match_sweep`)
//!
//! A run without `--play`, or with a test run's flag (`--input-script`, `--bench`, `--shot`, ...), mixes its sound
//! but sends silence to the device (`--sound` to hear it), and its window opens in the background: cloaked, never
//! activated, off the taskbar (`--foreground` to see it); `--mute` and `--background` do so for any run.
//!
//! `--bench <secs>` counts the frames from the first drawn one for that long (or to the match's end), prints one
//! `mvs: bench` line and leaves.

// a desktop program: no console window of its own (mvs_app::console)
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::ExitCode;

use mvs_app::app::{App, Go, MatchStart, RunOptions, Shell, ShellOptions};
use mvs_app::online::OnlineOptions;
use mvs_app::run::MatchOptions;
use mvs_app::script::ScriptOptions;
use mvs_net::link::relay::NetFaults;

/// `--net-cut` starts this long after the online match's go unless `--net-cut-at` says (s).
const NET_CUT_AT_S: f64 = 10.0;

const USAGE: &str = "mvs --version\nmvs --compat-id\nmvs --self-test [--data <dir>] [--store <dir>] [--check-out <file>]\nmvs --net-test [--data <dir>] [--online-defaults <file>] [--relay-only] [--direct-loopback]\nmvs --load-bench [--store <dir>] [--adapter <text>]\n\
mvs --play --store <dir> [--data <dir>] [--size 1920x1080] [--full-screen] [--adapter <text>] [--no-audio] [--mute | --sound] [--background | --foreground] [--online-defaults <file>]\n\
mvs [--data <dir>] [--store <dir>] [--size 1920x1080] [--full-screen] [--adapter <text>] [--seed <n>] [--no-ui] \
[--replay <trace match> [--trace-out <file>]] [--real-pace] [--shot <tick>[,..]] [--out <dir>] [--throttle <rate>] \
[--bench <secs>] [--stage <key>] [--stage-layout <file>] [--bots <level>] [--shot-results <secs>] [--no-audio] [--input-script <jsonl>] [--menu-tape <out>] [--shot-frames <frame>[,..]] \
[--live-loads] [--audio-tape <file>] [--waves endless|100 [--character <key>] [--waves-bot <level>]] [--wheel [--wheel-seconds <n>]]\n[--relay <addr> (--host | --join <lobby>) [--key <file>] [--feed <trace match>]]\n\
[--online-config <file> [--direct-loopback] [--online-open host|join:<code>|queue:<mode>[,..]] [--online-lag <ms>] \
[--online-desync-at <tick>] [--feed <trace match>]]
\n[--queue-fake [--queue-bots <s>] [--queue-start <s>] [--queue-vote-no] [--queue-path <pm.path json>]]\n\
[--fake-lobbies]\n\
[--net-cut <s> [--net-cut-at <s>]] [--net-loss <share>] [--net-delay <ms>] [--net-jitter <ms>]";

/// The exe's arguments.
struct Args {
    shell: ShellOptions,
    /// `--play`: the title first; else the command line's match, once.
    play: bool,
    /// `--data` and `--online-defaults` as given (a `--play` with no store hands them to the launcher).
    data_given: Option<PathBuf>,
    online_defaults_given: Option<PathBuf>,
    seed: i32,
    /// `--stage`: the stage key of the command line's match (an online host's too; the guest takes the host's).
    stage: String,
    /// (the port's own, dev) `--stage-layout <file>`: the command line's match on a custom stage, its layout's base
    /// the stage.
    layout: Option<std::sync::Arc<mvs_stage::Layout>>,
    replay: Option<String>,
    run: RunOptions,
    /// `--size` or `--full-screen` given: the window opens as they say, not as the settings saved it.
    window_given: bool,
    /// `--online-config <file>`: the menus' online play over the net service, its backends from that file over the
    /// player's (a dev run; a `--play` run without it signs in on the release's online.json).
    online_config: Option<PathBuf>,
    /// `--direct-loopback`: every listen, link and LAN announce on 127.0.0.1 alone (the two-exe runs).
    loopback: bool,
    /// `--online-open`: the session the menus open at their first frame (the dev runs'); `join:<code>` opens the
    /// lobby screen on the code (a join link's start), as the join box does.
    online_open: Option<mvs_ui::menu::online::OnlineCmd>,
    /// `--online-lag <ms>`: a test delay on this side's match packets.
    online_lag: f64,
    /// `--online-desync-at <tick>`: this side's state hash at that confirmed tick made wrong (the desync report's
    /// checks).
    online_desync_at: Option<u32>,
    /// `--net-cut`, `--net-cut-at`, `--net-loss`, `--net-delay`, `--net-jitter`: the relay client's faults.
    net_faults: NetFaults,
    online_feed: Option<String>,
    /// (dev) `--queue-fake`: the queue with no server, its options; an input script's
    /// `{"query": ..}` line sets the same (queue=fake, queueBots, queueStart, queueVote=no).
    queue_fake: Option<mvs_app::online::fake_queue::FakeOptions>,
    /// (dev) `--fake-lobbies`: the custom games list shows this many made-up lobbies.
    fake_lobbies: Option<u32>,
    /// `--queue-bots <s>`: the wait for bots each queue join asks for.
    queue_bots: Option<f64>,
    /// `--load-bench`: the loads timed (`mvs_app::load_bench`), then the program leaves.
    load_bench: bool,
    /// `--match-sweep`: every fighter and skin through a custom game's load and first ticks (`mvs_app::match_sweep`).
    sweep: Option<mvs_app::match_sweep::SweepOptions>,
    /// The sound mixed but not heard: `--mute`, a run without `--play` or a test run's flag (`TEST_FLAGS`), unless
    /// `--sound`.
    muted: bool,
    /// The window opened in the background (`platform::set_background`): the same runs as `muted`, unless
    /// `--foreground`; `--background` for any run.
    background: bool,
    /// `--audio-tape <file>`: every call any screen hands the sound engine written to this file from the program's
    /// start (the post tape's lines: `mvs_app::audio::TapeWriter`).
    audio_tape: Option<PathBuf>,
    /// (the port's own) `--waves endless|100`: the command line's match is a Waves run (`mvs_app::waves`).
    waves: Option<mvs_sim::setup::WavesVariant>,
    /// `--character <key>`: the Waves run's fighter (None: the default one).
    character: Option<String>,
    /// (dev) `--waves-bot <level>`: a match bot of that level in the Waves run's player seat (a run that lasts).
    waves_bot: Option<u8>,
    /// (dev) `--bots <level>`: every player of the command line's match a match bot of that level (a match that
    /// plays itself, on a custom stage's layout say).
    bots: Option<u8>,
    /// (the port's own, dev) `--wheel`: the Chaos Wheel rule in the matches of `mvs_app::wheel::switched`.
    wheel: bool,
    /// (dev) `--wheel-seconds <n>`: `--wheel` with a window of that many seconds (a picture of a change needs no
    /// 20 seconds' wait).
    wheel_seconds: Option<f64>,
}

/// The flags of a test or scripted run: one of them mutes the device unless `--sound` is given, so a run the agents
/// make does not play over the user's own work.
const TEST_FLAGS: [&str; 12] = [
    "--input-script",
    "--menu-tape",
    "--shot-frames",
    "--bench",
    "--shot",
    "--shot-results",
    "--press-results",
    "--replay",
    "--throttle",
    "--direct-loopback",
    "--queue-fake",
    "--online-config",
];

/// `--online-open`'s value as the menus' command.
fn online_open_of(v: &str) -> Result<mvs_ui::menu::online::OnlineCmd, String> {
    use mvs_net::service::api::SessionTarget;
    use mvs_ui::menu::online::OnlineCmd;
    if v == "host" {
        return Ok(OnlineCmd::Open(SessionTarget::Host));
    }
    if let Some(code) = v.strip_prefix("join:") {
        return Ok(OnlineCmd::Open(SessionTarget::Join(code.to_owned())));
    }
    if let Some(list) = v.strip_prefix("queue:") {
        let modes = list
            .split(',')
            .map(|m| mvs_protocol::online::QueueMode::of(m).ok_or(format!("--online-open: no queue mode {m}")))
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(OnlineCmd::Queue { modes });
    }
    Err(format!("--online-open: {v} is not host, join:<code> or queue:<modes>"))
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let num = |v: Option<String>, what: &str| -> Result<u32, String> {
        let v = v.ok_or(format!("{what}: no value"))?;
        v.parse().map_err(|_| format!("{what}: {v} is not a count"))
    };
    let secs = |v: Option<String>, what: &str| -> Result<f64, String> {
        let v = v.ok_or(format!("{what}: no value"))?;
        v.parse::<f64>()
            .ok()
            .filter(|x| x.is_finite() && *x > 0.0)
            .ok_or(format!("{what}: {v} is not a finite positive number"))
    };
    let at_least_0 = |v: Option<String>, what: &str| -> Result<f64, String> {
        let v = v.ok_or(format!("{what}: no value"))?;
        v.parse::<f64>()
            .ok()
            .filter(|x| x.is_finite() && *x >= 0.0)
            .ok_or(format!("{what}: {v} is not a finite number of 0 or more"))
    };
    let mut store = None;
    let mut data = None;
    let mut online = OnlineOptions::default();
    let mut relay = None;
    let mut a = Args {
        shell: ShellOptions {
            store: None,
            data: PathBuf::new(),
            size: [1920, 1080],
            full_screen: false,
            adapter: None,
            ui: true,
            audio: true,
            audio_offline: false,
            online_defaults: std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| mvs_release::online_json(&mvs_release::app_root(d))))
                .unwrap_or_else(|| PathBuf::from(mvs_release::ONLINE_JSON)),
        },
        play: false,
        data_given: None,
        online_defaults_given: None,
        seed: 1,
        stage: mvs_sim_trace::TRACE_STAGE.to_owned(),
        layout: None,
        replay: None,
        run: RunOptions {
            first: MatchOptions::default(),
            results_press: None,
            once: true,
            bench: None,
            script: ScriptOptions::default(),
            online: None,
        },
        window_given: false,
        online_config: None,
        loopback: false,
        online_open: None,
        online_lag: 0.0,
        online_desync_at: None,
        net_faults: NetFaults { cut_at_s: NET_CUT_AT_S, ..NetFaults::default() },
        online_feed: None,
        queue_fake: None,
        fake_lobbies: None,
        queue_bots: None,
        load_bench: false,
        sweep: None,
        muted: false,
        background: false,
        audio_tape: None,
        waves: None,
        character: None,
        waves_bot: None,
        bots: None,
        wheel: false,
        wheel_seconds: None,
    };
    let (mut mute, mut sound, mut test, mut back, mut front) = (false, false, false, false, false);
    let (mut audio_tape, mut results_press) = (None, None);
    let o = &mut a.run.first;
    while let Some(arg) = it.next() {
        test |= TEST_FLAGS.contains(&arg.as_str());
        match arg.as_str() {
            "--play" => a.play = true,
            "--mute" => mute = true,
            "--sound" => sound = true,
            "--background" => back = true,
            "--foreground" => front = true,
            "--load-bench" => {
                a.load_bench = true;
                a.shell.audio_offline = true;
                a.shell.size = [640, 360];
            }
            "--match-sweep" => {
                a.sweep = Some(mvs_app::match_sweep::SweepOptions::default());
                a.shell.audio_offline = true;
                a.shell.size = [640, 360];
            }
            "--sweep-ticks" => {
                let n = num(it.next(), "--sweep-ticks")?;
                a.sweep.get_or_insert_with(Default::default).ticks = n;
            }
            "--sweep-only" => {
                let v = it.next().ok_or("--sweep-only: no value")?;
                a.sweep.get_or_insert_with(Default::default).only = v.split(',').map(str::to_owned).collect();
            }
            "--data" => data = Some(PathBuf::from(it.next().ok_or("--data: no value")?)),
            "--store" => store = Some(PathBuf::from(it.next().ok_or("--store: no value")?)),
            "--size" => {
                let v = it.next().ok_or("--size: no value")?;
                let (w, h) = v.split_once('x').ok_or(format!("--size: {v} is not WxH"))?;
                a.shell.size = [num(Some(w.to_string()), "--size")?, num(Some(h.to_string()), "--size")?];
                a.window_given = true;
            }
            "--full-screen" => {
                a.shell.full_screen = true;
                a.window_given = true;
            }
            "--adapter" => a.shell.adapter = Some(it.next().ok_or("--adapter: no value")?),
            "--seed" => {
                let v = it.next().ok_or("--seed: no value")?;
                a.seed = v.parse().map_err(|_| format!("--seed: {v} is not a number"))?;
            }
            "--no-ui" => a.shell.ui = false,
            "--no-audio" => a.shell.audio = false,
            "--stage" => a.stage = it.next().ok_or("--stage: no value")?,
            "--stage-layout" => {
                let path = it.next().ok_or("--stage-layout: no value")?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("--stage-layout {path}: {e}"))?;
                let l = mvs_stage::Layout::read(&text).map_err(|e| format!("--stage-layout {path}: {e}"))?;
                mvs_stage::check(&l).map_err(|e| format!("--stage-layout {path}: {e}"))?;
                a.layout = Some(std::sync::Arc::new(l));
            }
            "--replay" => a.replay = Some(it.next().ok_or("--replay: no value")?),
            "--waves" => {
                let v = it.next().ok_or("--waves: no value")?;
                a.waves = Some(mvs_app::waves::variant_of(&v).ok_or(format!("--waves: {v} is not endless or 100"))?);
            }
            "--character" => {
                let v = it.next().ok_or("--character: no value")?;
                if !mvs_protocol::roster::CHARACTERS.contains(&v.as_str()) {
                    return Err(format!("--character: {v} is not a roster key"));
                }
                a.character = Some(v);
            }
            "--bots" => {
                let v = it.next().ok_or("--bots: no value")?;
                a.bots = Some(v.parse().map_err(|_| format!("--bots: {v} is not a bot level"))?);
            }
            "--waves-bot" => {
                let v = it.next().ok_or("--waves-bot: no value")?;
                a.waves_bot = Some(v.parse().map_err(|_| format!("--waves-bot: {v} is not a bot level"))?);
            }
            "--wheel" => a.wheel = true,
            "--wheel-seconds" => {
                a.wheel = true;
                a.wheel_seconds = Some(secs(it.next(), "--wheel-seconds")?);
            }
            "--shot" => {
                for t in it.next().ok_or("--shot: no value")?.split(',') {
                    o.shots.push(num(Some(t.to_string()), "--shot")?);
                }
            }
            "--out" => o.out = PathBuf::from(it.next().ok_or("--out: no value")?),
            "--trace-out" => o.trace_out = Some(PathBuf::from(it.next().ok_or("--trace-out: no value")?)),
            "--audio-tape" => audio_tape = Some(PathBuf::from(it.next().ok_or("--audio-tape: no value")?)),
            "--press-results" => {
                let v = it.next().ok_or("--press-results: no value")?;
                results_press = Some(v.parse().map_err(|_| format!("--press-results: {v} is not a number"))?);
            }
            "--shot-results" => {
                let v = it.next().ok_or("--shot-results: no value")?;
                o.results_shot = Some(v.parse().map_err(|_| format!("--shot-results: {v} is not a number"))?);
            }
            "--bench" => a.run.bench = Some(secs(it.next(), "--bench")?),
            "--throttle" => o.throttle = secs(it.next(), "--throttle")?.max(1.0),
            "--real-pace" => o.real_pace = true,
            "--relay" => relay = Some(it.next().ok_or("--relay: no value")?),
            "--host" => online.host = true,
            "--join" => online.join = Some(it.next().ok_or("--join: no value")?),
            "--key" => online.key = Some(PathBuf::from(it.next().ok_or("--key: no value")?)),
            "--feed" => online.feed = Some(it.next().ok_or("--feed: no value")?),
            "--online-defaults" => {
                a.shell.online_defaults = PathBuf::from(it.next().ok_or("--online-defaults: no value")?);
                a.online_defaults_given = Some(a.shell.online_defaults.clone());
            }
            "--online-config" => a.online_config = Some(PathBuf::from(it.next().ok_or("--online-config: no value")?)),
            "--direct-loopback" => a.loopback = true,
            "--online-open" => a.online_open = Some(online_open_of(&it.next().ok_or("--online-open: no value")?)?),
            "--online-lag" => {
                let v = it.next().ok_or("--online-lag: no value")?;
                a.online_lag = v.parse().map_err(|_| format!("--online-lag: {v} is not a number"))?;
            }
            "--online-desync-at" => {
                let v = it.next().ok_or("--online-desync-at: no value")?;
                a.online_desync_at = Some(v.parse().map_err(|_| format!("--online-desync-at: {v} is not a tick"))?);
            }
            "--net-cut" => a.net_faults.cut_s = secs(it.next(), "--net-cut")?,
            "--net-cut-at" => a.net_faults.cut_at_s = at_least_0(it.next(), "--net-cut-at")?,
            "--net-loss" => {
                let v = at_least_0(it.next(), "--net-loss")?;
                if v > 1.0 {
                    return Err(format!("--net-loss: {v} is over 1"));
                }
                a.net_faults.loss = v;
            }
            "--net-delay" => a.net_faults.delay_ms = at_least_0(it.next(), "--net-delay")?,
            "--net-jitter" => a.net_faults.jitter_ms = at_least_0(it.next(), "--net-jitter")?,
            "--queue-fake" => {
                a.queue_fake.get_or_insert_with(Default::default);
            }
            "--queue-bots" => a.queue_bots = Some(secs(it.next(), "--queue-bots")?),
            "--fake-lobbies" => a.fake_lobbies = Some(10),
            "--queue-start" => {
                a.queue_fake.get_or_insert_with(Default::default).start_s = secs(it.next(), "--queue-start")?
            }
            "--queue-vote-no" => a.queue_fake.get_or_insert_with(Default::default).vote_no = true,
            "--queue-path" => {
                let v = it.next().ok_or("--queue-path: no value")?;
                let m = serde_json::from_str::<mvs_protocol::messages::ServerMessage>(&v)
                    .ok()
                    .filter(|m| matches!(m, mvs_protocol::messages::ServerMessage::PmPath { .. }))
                    .ok_or("--queue-path: not a pm.path message")?;
                a.queue_fake.get_or_insert_with(Default::default).path = Some(m);
            }
            _ if a.run.script.flag(&arg, &mut it)? => {}
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if a.play && a.stage != mvs_sim_trace::TRACE_STAGE {
        return Err("--stage: not with --play (the menus pick the stage from the map)".to_owned());
    }
    if a.layout.is_some() && (a.play || a.replay.is_some() || a.waves.is_some() || relay.is_some()) {
        return Err("--stage-layout: not with --play, --replay, --waves or --relay".to_owned());
    }
    if let Some(l) = &a.layout {
        // (the layout names its base: the stage loaded under it)
        a.stage.clone_from(&l.base);
    }
    if a.replay.is_some() && a.stage != mvs_sim_trace::TRACE_STAGE {
        return Err("--stage: not with --replay (the trace's header names the stage)".to_owned());
    }
    if a.waves.is_some() && (a.play || a.replay.is_some() || relay.is_some() || a.run.script.input_script.is_some()) {
        return Err("--waves: not with --play, --replay, --relay or --input-script".to_owned());
    }
    if a.wheel && (a.waves.is_some() || a.replay.is_some()) {
        return Err("--wheel: not with --waves or --replay".to_owned());
    }
    if a.bots.is_some() && (a.play || a.replay.is_some() || a.waves.is_some() || relay.is_some()) {
        return Err("--bots: not with --play, --replay, --waves or --relay".to_owned());
    }
    if (a.character.is_some() || a.waves_bot.is_some()) && a.waves.is_none() {
        return Err("--character, --waves-bot: no --waves".to_owned());
    }
    if o.trace_out.is_some() && a.replay.is_none() {
        return Err("--trace-out: no --replay".to_owned());
    }
    if o.real_pace && a.replay.is_none() && a.run.script.input_script.is_none() {
        return Err("--real-pace: no --replay or --input-script".to_owned());
    }
    a.run.script.check()?;
    a.audio_tape = audio_tape;
    a.run.results_press = results_press;
    let test = test || !a.play;
    a.muted = (mute || test) && !sound;
    a.background = (back || test) && !front;
    // (an online match: a relay and one way into its lobby)
    match relay {
        Some(r) => {
            if online.host == online.join.is_some() {
                return Err("--relay: --host or --join <lobby>, one of them".to_owned());
            }
            a.run.online = Some(OnlineOptions { relay: r, faults: a.net_faults, ..online.clone() });
        }
        // (--feed alone: a menus' online match's, with --online-config)
        None if a.online_config.is_some()
            && online == OnlineOptions { feed: online.feed.clone(), ..Default::default() } => {}
        None if online != OnlineOptions::default() => {
            return Err("--host, --join, --key, --feed: no --relay".to_owned());
        }
        None => {}
    }
    if a.online_config.is_none() && (a.loopback || a.online_lag != 0.0 || a.online_desync_at.is_some()) {
        return Err("--direct-loopback, --online-lag, --online-desync-at: no --online-config".to_owned());
    }
    // (a player's run takes a join link's `--online-open join:<code>` from the launcher)
    if a.online_config.is_none() && !a.play && a.online_open.is_some() {
        return Err("--online-open: no --online-config or --play".to_owned());
    }
    if a.online_config.is_some() && a.run.online.is_some() {
        return Err("--online-config: not with --relay".to_owned());
    }
    if let Some(path) = &a.run.script.input_script {
        // (the script's queue options)
        let s = mvs_app::script::Script::read(path).map_err(|e| e.to_string())?;
        let q: Vec<(&str, &str)> =
            s.query.trim_start_matches('?').split('&').map(|kv| kv.split_once('=').unwrap_or((kv, ""))).collect();
        let get = |k: &str| q.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        if get("queue") == Some("fake") {
            let f = a.queue_fake.get_or_insert_with(Default::default);
            if let Some(v) = get("queueStart").and_then(|v| v.parse().ok()) {
                f.start_s = v;
            }
            f.vote_no |= get("queueVote") == Some("no");
        }
        if let Some(v) = get("queueBots").and_then(|v| v.parse::<f64>().ok()) {
            a.queue_bots = Some(v);
        }
    }
    if a.fake_lobbies.is_some() && !a.play {
        return Err("--fake-lobbies: only with --play".to_owned());
    }
    if (a.queue_fake.is_some() || a.queue_bots.is_some()) && !a.play {
        return Err(
            "--queue-fake, --queue-bots (or the input script's {\"query\"} line with queue=fake / queueBots): only with --play"
                .to_owned(),
        );
    }
    if a.online_config.is_some() && !a.play {
        return Err("--online-config: only with --play (the menus' online play)".to_owned());
    }
    a.online_feed = online.feed.clone().filter(|_| a.online_config.is_some());
    a.shell.data = mvs_settings::data_dir(data.as_deref());
    a.data_given = data;
    if a.play {
        a.run.once = false;
        a.shell.store = store;
        if !a.window_given {
            // The window opens as it was left: its mode and its size, placed by
            // `Shell::place_window` once it is made.
            let w = mvs_settings::Settings::load(&a.shell.data).window;
            a.shell.size = if w.width > 0 && w.height > 0 { [w.width, w.height] } else { mvs_app::platform::FIRST_RUN };
            a.shell.full_screen = w.mode == mvs_settings::WindowMode::WindowedFullscreen;
        }
    } else {
        a.shell.store = Some(store.unwrap_or_else(mvs_store::dev_store_dir));
    }
    Ok(a)
}

/// A panic's time (UTC), message, place and backtrace appended to `crash.log` in the data folder (beside settings.json), as well as
/// printed: a run started from Explorer closes its console with the message.
fn log_panics(data: &std::path::Path) {
    let dir = data.to_path_buf();
    let printed = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        printed(info);
        let thread = std::thread::current().name().unwrap_or("?").to_owned();
        let trace = std::backtrace::Backtrace::force_capture();
        let at = mvs_app::import::serve::iso_utc(std::time::SystemTime::now());
        let line = format!("{} at {at}\nthread {thread}: {info}\n{trace}\n", mvs_release::version_line());
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
        }
    }));
}

fn run(a: Args) -> mvs_app::Result<()> {
    log_panics(&a.shell.data);
    // a player's run: online for the app's life on the release's online.json (below), the data folder's game for
    // join links from its first moment (a link's launcher looks for it while the window opens)
    let player_run = a.online_config.is_none()
        && a.play
        && !a.run.script.active()
        && a.queue_fake.is_none()
        && a.queue_bots.is_none();
    // (held to the run's end)
    let _instance = if player_run { mvs_app::online::link::hold_instance(&a.shell.data) } else { None };
    // (before the store opens: its base scope is the tape's first line)
    if let Some(path) = &a.audio_tape {
        mvs_app::audio::TapeWriter::open(path)?;
    }
    // (an online match meets its other player before the window opens)
    let online = match &a.run.online {
        Some(o) => Some(mvs_app::online::connect(o, &a.shell.data, a.seed, &a.stage)?),
        None => None,
    };
    let mut shell = Shell::new(&a.shell)?;
    shell.wheel = a.wheel.then(|| mvs_app::wheel::switch_rule(a.wheel_seconds));
    if a.load_bench {
        return mvs_app::load_bench::run(&mut shell);
    }
    if let Some(o) = &a.sweep {
        return mvs_app::match_sweep::run(&mut shell, o);
    }
    if a.play && !a.window_given {
        shell.place_window()?;
    }
    if let Some(file) = &a.online_config {
        let (net, hub) = mvs_app::online::glue::start(
            file,
            &shell.settings,
            &shell.data,
            a.loopback,
            true,
            String::new(),
            a.online_lag,
            a.net_faults,
        );
        // (run at the menus' first frame, once the compatibility id is made; a join opens the lobby screen)
        if let Some(c) = a.online_open.clone() {
            hub.borrow_mut().push_open(c);
        }
        hub.borrow_mut().desync_at = a.online_desync_at;
        shell.net = Some(net);
        shell.online = Some(hub);
        shell.online_feed.clone_from(&a.online_feed);
    } else if player_run {
        // a player's run: online for the app's life, on the release's
        // online.json with the player's own settings over it
        let defaults = shell.online_defaults.clone();
        let (net, hub) = mvs_app::online::glue::start(
            &defaults,
            &shell.settings,
            &shell.data,
            false,
            false,
            String::new(),
            0.0,
            a.net_faults,
        );
        // (a join link's join: the lobby screen on its code, once the sign-in and the gate let it)
        if let Some(c) = a.online_open.clone() {
            hub.borrow_mut().push_open(c);
        }
        shell.net = Some(net);
        shell.online = Some(hub);
    }
    if a.queue_fake.is_some() || a.queue_bots.is_some() {
        // (the fake queue needs no service: a client with no port when there is none)
        let hub = shell.online.get_or_insert_with(|| mvs_app::online::glue::start_offline(&shell.data)).clone();
        let mut h = hub.borrow_mut();
        h.fake = a.queue_fake.clone().map(mvs_app::online::fake_queue::FakeCore::new);
        h.bot_after = a.queue_bots;
    }
    if a.fake_lobbies.is_some() {
        let hub = shell.online.get_or_insert_with(|| mvs_app::online::glue::start_offline(&shell.data)).clone();
        hub.borrow_mut().fake_lobbies =
            a.fake_lobbies.map(|count| mvs_app::online::fake_lobbies::FakeLobbies { count, icons: Vec::new() });
    }
    let first = if a.run.script.active() {
        Go::Title
    } else if let Some(start) = online {
        Go::Match(Box::new(start))
    } else if a.play {
        Go::Title
    } else {
        Go::Match(Box::new(match &a.replay {
            Some(name) => MatchStart::replay(name),
            None => match a.waves {
                Some(variant) => {
                    let roster =
                        shell.ui.as_ref().and_then(|u| u.menu.menu.as_ref()).map_or(&[][..], |m| m.roster.as_slice());
                    let character = a.character.as_deref();
                    let (settings, bot) = (&shell.settings, a.waves_bot);
                    mvs_app::waves::waves_match(variant, character, bot, settings, roster, a.seed, &a.stage)
                }
                None => {
                    let mut start = MatchStart::default_match(a.seed);
                    if let Some(level) = a.bots {
                        for p in &mut start.setup.players {
                            p.bot = true;
                            p.difficulty = Some(mvs_sim::Real::from(level));
                        }
                    }
                    start.stage.clone_from(&a.stage);
                    start.layout.clone_from(&a.layout);
                    start
                }
            },
        }))
    };
    let mut app = App::new(shell, first, &a.run)?;
    let r = app.run(&a.run);
    // (the window closed first: an online match's end may still wait for the other player)
    drop(app);
    mvs_app::online::settle();
    r
}

/// `--play` with no store: the release's launcher started with `--play` (the game
/// alone when the store is ready, else the launcher), this program leaving.
fn to_launcher(a: &Args) -> ExitCode {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| mvs_release::launcher_exe(&mvs_release::app_root(d))));
    let Some(exe) = exe.filter(|e| e.is_file()) else {
        eprintln!("mvs: --play needs --store <dir>: start the game from {}", mvs_release::LAUNCHER_EXE);
        return ExitCode::from(2);
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--play");
    if let Some(d) = &a.data_given {
        cmd.arg("--data").arg(d);
    }
    if let Some(f) = &a.online_defaults_given {
        cmd.arg("--online-defaults").arg(f);
    }
    if let Some(g) = &a.shell.adapter {
        cmd.arg("--adapter").arg(g);
    }
    match cmd.spawn() {
        Ok(_) => {
            eprintln!("mvs: --play with no --store: {} starts the game", exe.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mvs: {} did not start: {e}", exe.display());
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    mvs_app::console::attach_parent();
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_some_and(|a| a == "--import-worker") {
        let op = args.nth(1).unwrap_or_default();
        return ExitCode::from(u8::try_from(mvs_app::import::worker_main(&op)).unwrap_or(1));
    }
    // The updater's jobs, with no window (mvs_app::update).
    if args.peek().is_some_and(|a| a == "--update-now") {
        return ExitCode::from(mvs_app::update::update_now_main());
    }
    if args.peek().is_some_and(|a| a == "--finish-update") {
        let job: Vec<String> = args.skip(1).collect();
        let pid = match job.get(1).map(String::as_str) {
            None => None,
            Some("--wait-pid") => job.get(2).and_then(|p| p.parse().ok()),
            Some(_) => None,
        };
        let (Some(target), true) = (job.first(), job.len() == 1 || pid.is_some() && job.len() == 3) else {
            eprintln!("mvs: usage: mvs --finish-update <dir> [--wait-pid <pid>]");
            return ExitCode::from(2);
        };
        return ExitCode::from(mvs_app::update::finish_update_main(std::path::Path::new(target), pid));
    }
    // (before anything opens: no window, no store, no settings read)
    if args.peek().is_some_and(|a| a == "--version") {
        println!("{}", mvs_release::version_line());
        return ExitCode::SUCCESS;
    }
    // (the compatibility id a fresh import gets: the release's reference file must be for it)
    if args.peek().is_some_and(|a| a == "--compat-id") {
        return match mvs_app::online::glue::code_compat_id() {
            Ok(id) => {
                println!("{id}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mvs: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.peek().is_some_and(|a| a == "--self-test") {
        return ExitCode::from(mvs_app::selftest::main(args.skip(1)));
    }
    if args.peek().is_some_and(|a| a == "--net-test") {
        return ExitCode::from(mvs_app::launcher::net_test::main(args.skip(1)));
    }
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("mvs: {e}\nusage: {USAGE}");
            return ExitCode::from(2);
        }
    };
    if a.play && a.shell.store.is_none() {
        return to_launcher(&a);
    }
    mvs_audio::out::wasapi::set_muted(a.muted);
    mvs_app::platform::set_background(a.background);
    match run(a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mvs: {e}");
            ExitCode::FAILURE
        }
    }
}
