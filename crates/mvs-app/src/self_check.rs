//! (the port's own) The self check: at the start this copy runs a short fixed match with seeded bots on a thread of
//! its own, with its own store, and compares the final world's hash with the release's (`mvs_release::SELF_CHECK_JSON`
//! beside the program, keyed by the compatibility id). A PC whose hash differs plays online matches that go out of
//! sync: its online play stays off (`Gate::SelfCheck`) before it meets anyone. No reference file, or one of another
//! compatibility id: no check, online stays on.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use mvs_data::GameData;
use mvs_net::session::wire_hash;
use mvs_release::{SELF_CHECK_JSON, SelfCheckRef};
use mvs_sim::ai::bt::BtBot;
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, Rules, TeamStyle};
use mvs_sim::{MatchSetup, World};
use mvs_store::Store;

use crate::online::hub::OnlineHub;

/// The check match's length (10 s of play) and its World seed.
pub const CHECK_TICKS: u32 = 600;
const CHECK_SEED: i32 = 9;
/// The two fighters, each a match bot of this level, seeded CHECK_BOT_SEED + its slot.
const CHECK_FIGHTERS: [&str; 2] = ["shaggy", "batman"];
const CHECK_BOT_LEVEL: i32 = 3;
const CHECK_BOT_SEED: i32 = 300;

/// A self check under way (`OnlineHub::self_check`), polled by `poll`.
#[derive(Debug)]
pub struct Running {
    answer: Receiver<Result<u32, String>>,
    /// The release's hash.
    release: u32,
}

/// The check match: a 1v1 of CHECK_FIGHTERS, both bots, with no intro (the fight from tick 0).
fn check_setup() -> MatchSetup {
    MatchSetup {
        team_style: TeamStyle::OneVsOne,
        players: CHECK_FIGHTERS
            .iter()
            .map(|c| MatchPlayer {
                bot: true,
                difficulty: Some(mvs_sim::real(f64::from(CHECK_BOT_LEVEL))),
                character: Some((*c).to_owned()),
                ..MatchPlayer::default()
            })
            .collect(),
        rules: Some(Rules { knockouts_to_win: mvs_sim::real(99.0), duration: mvs_sim::real(420.0), stocks: false }),
        allow_hazards: true,
        ..MatchSetup::default()
    }
}

/// The fixed check match on `store`: the wire hash (mvs_net::session::wire_hash) of its final world.
pub fn run(store: &Store) -> Result<u32, String> {
    let setup = check_setup();
    let keys = mvs_sim_trace::keys_of(&setup);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let data =
        GameData::load(store, &refs, mvs_sim_trace::TRACE_STAGE).map_err(|e| format!("the gamedata kind: {e:?}"))?;
    let trees = Arc::new(data.bot_trees.clone());
    let mut w = World::new(Arc::new(data), CHECK_SEED, &setup).map_err(|e| format!("the bind: {e:?}"))?;
    w.bot_trees = Some(trees.clone());
    let mut bots: Vec<BtBot> = (0..CHECK_FIGHTERS.len())
        .map(|i| BtBot::match_bot(CHECK_BOT_SEED + i32::try_from(i).expect("two slots"), &trees, CHECK_BOT_LEVEL))
        .collect();
    let mut inputs = vec![NO_INPUT; bots.len()];
    for _ in 0..CHECK_TICKS {
        for (slot, bot) in bots.iter_mut().enumerate() {
            let me = w.players[slot];
            inputs[slot] = bot.think(&mut w, me);
        }
        w.step(&inputs);
        w.events.clear();
    }
    Ok(wire_hash(mvs_sim::state::hash_of(&w)))
}

/// The release's hash for `compat` from the reference file in `dir` (the program's folder); None: no file, another
/// compatibility id, or a file that does not read.
pub fn reference(dir: &Path, compat: &str) -> Option<u32> {
    let text = std::fs::read(dir.join(SELF_CHECK_JSON)).ok()?;
    let r: SelfCheckRef = serde_json::from_slice(&text).ok()?;
    (r.compat == compat).then_some(r.hash)
}

/// The program's folder (where the release puts SELF_CHECK_JSON).
fn program_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// The calling thread at the lowest priority: the check never takes time from the game's start.
fn lowest_priority() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_LOWEST};
        // SAFETY: GetCurrentThread's pseudo handle is always valid for the calling thread, and SetThreadPriority
        // only reads it; a failure leaves the priority as it was.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };
    }
}

/// The check of the store at `dir`, on a thread of its own at the lowest priority: its answer comes on the receiver
/// (a panic in the check: the sender dropped).
pub fn spawn(dir: &Path) -> Receiver<Result<u32, String>> {
    let (tx, rx) = channel();
    let dir = dir.to_path_buf();
    let spawned = std::thread::Builder::new().name("self-check".to_owned()).spawn(move || {
        lowest_priority();
        let r = Store::open(&dir).map_err(|e| format!("the store {}: {e}", dir.display())).and_then(|s| run(&s));
        let _ = tx.send(r);
    });
    if let Err(e) = spawned {
        eprintln!("mvs: self check: no thread: {e}");
    }
    rx
}

/// Starts the self check of the store at `store` for `compat` (once the compatibility id is made) when there is a
/// reference to compare with: `OnlineHub::self_check` while it runs (online::gate::poll says Checking meanwhile).
pub fn begin(hub: &mut OnlineHub, store: &Path, compat: &str) {
    begin_in(hub, store, compat, program_dir().as_deref());
}

/// `begin` with the reference read from `dir` (None: no program folder).
pub fn begin_in(hub: &mut OnlineHub, store: &Path, compat: &str, dir: Option<&Path>) {
    let Some(release) = dir.and_then(|d| reference(d, compat)) else {
        eprintln!("mvs: self check: no reference for {compat} in {SELF_CHECK_JSON} beside the program: not run");
        return;
    };
    hub.self_check_differs = None;
    hub.self_check = Some(Running { answer: spawn(store), release });
}

/// The self check's answer once it is in: `OnlineHub::self_check` cleared, and `self_check_differs` set when the
/// hash differs from the release's (online::gate::poll turns it into Gate::SelfCheck). Each frame of the menus.
pub fn poll(hub: &mut OnlineHub) {
    let Some(running) = &hub.self_check else { return };
    let answer = match running.answer.try_recv() {
        Ok(a) => a,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err("the check stopped before its end".to_owned()),
    };
    let release = running.release;
    hub.self_check = None;
    hub.self_check_differs = match answer {
        Ok(mine) if mine == release => {
            eprintln!("mvs: self check: {mine:08x}, the release's");
            None
        }
        Ok(mine) => {
            eprintln!("mvs: self check: {mine:08x}, the release's {release:08x}: online play stays off");
            Some((mine, release))
        }
        // Unverified: a check that could not run (a store that does not open, a panic) keeps online play on, as a
        // copy without a reference does: the match's own hash checks still find a desync.
        Err(e) => {
            eprintln!("mvs: self check: not run ({e}): online play stays on");
            None
        }
    };
}

/// A self check is under way (online::gate::poll says Checking meanwhile).
pub fn running(hub: &OnlineHub) -> bool {
    hub.self_check.is_some()
}

/// (mine, release) once a finished check's hash differed from the release's; None while it runs, when it passed,
/// could not run, or had no reference.
pub fn differs(hub: &OnlineHub) -> Option<(u32, u32)> {
    hub.self_check_differs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::port::NetPort;
    use crate::online::tables::Tables;

    fn hub() -> OnlineHub {
        OnlineHub::new(NetPort::new(None), None, "c1".into(), Tables::default(), 0.0)
    }

    /// The check's answer, waited for.
    fn answer(h: &mut OnlineHub) {
        let start = std::time::Instant::now();
        while running(h) {
            assert!(start.elapsed().as_secs() < 300, "the self check answers");
            std::thread::sleep(std::time::Duration::from_millis(20));
            poll(h);
        }
    }

    fn with_reference(dir: &Path, compat: &str, hash: u32) {
        let r = SelfCheckRef { compat: compat.to_owned(), hash };
        std::fs::write(dir.join(SELF_CHECK_JSON), serde_json::to_string(&r).unwrap()).unwrap();
    }

    #[test]
    fn the_reference_is_read_by_its_compat() {
        let dir = std::env::temp_dir().join(format!("mvs-self-check-ref-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(reference(&dir, "c1"), None);
        with_reference(&dir, "c1", 0x1234);
        assert_eq!(reference(&dir, "c1"), Some(0x1234));
        assert_eq!(reference(&dir, "c2"), None);
        std::fs::write(dir.join(SELF_CHECK_JSON), "{").unwrap();
        assert_eq!(reference(&dir, "c1"), None);
        // no reference: nothing runs, online play stays on
        let mut h = hub();
        begin_in(&mut h, &dir.join("no-store"), "c1", Some(&dir));
        assert!(!running(&h));
        assert_eq!(differs(&h), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The check match twice gives one hash; the release's reference that hash: online on; a changed one: off; a
    /// store that does not open: on.
    #[test]
    fn the_check_is_the_same_each_run_and_a_changed_reference_turns_online_off() {
        let store_dir = mvs_store::dev_store_dir();
        let Ok(store) = Store::open(&store_dir) else {
            eprintln!("note: no store at {}; nothing checked", store_dir.display());
            return;
        };
        let a = run(&store).expect("the check match runs");
        let b = run(&store).expect("the check match runs");
        assert_eq!(a, b, "the check match is deterministic");
        let dir = std::env::temp_dir().join(format!("mvs-self-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let compat = crate::online::glue::compat_id(store.manifest());
        with_reference(&dir, &compat, a);
        let mut h = hub();
        begin_in(&mut h, &store_dir, &compat, Some(&dir));
        assert!(running(&h));
        answer(&mut h);
        assert_eq!(differs(&h), None);
        with_reference(&dir, &compat, a ^ 1);
        begin_in(&mut h, &store_dir, &compat, Some(&dir));
        answer(&mut h);
        assert_eq!(differs(&h), Some((a, a ^ 1)));
        begin_in(&mut h, &dir.join("no-store"), &compat, Some(&dir));
        answer(&mut h);
        assert_eq!(differs(&h), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
