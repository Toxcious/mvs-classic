//! A path no trace match runs: Shaggy_Heavy_U_Montage's BranchWindow_C_0 (1.178-1.375 s, queued, not Immediate,
//! CanBranchToSameAttack false), whose one row is the montage itself (heavy, Up; its group
//! BranchGroup_ShaggyBase+Neutral1 is not in the data), holding a stored up attack when a stop before the frame's
//! dispatch ends the montage. The late window end still finds the stopped attack as the current one
//! (OnMontageBlendingOut @1802-2111 keeps CurrentMontage while the maps hold the window), so IsBranchValid @49
//! refuses the self branch and nothing starts.

use mvs_sim::fighter::anim::QueuedBtn;
use mvs_sim::fighter::anim::branches::buffer_press;
use mvs_sim::fighter::anim::montages::stop_montage;
use mvs_sim::input::{Btn, Dir, NO_INPUT};
use mvs_sim_trace::{harness, run};

const MONTAGE: &str = "Shaggy_Heavy_U_Montage";
const MATCHES: [&str; 6] = ["m1-smoke", "batman-shaggy", "arya-shaggy", "bugs-shaggy", "lebron-shaggy", "m2"];

fn montage_of(h: &harness::Harness, f: mvs_sim::fighter::FighterId) -> (String, f64) {
    h.w.fighter(f)
        .montages
        .montage
        .as_ref()
        .map_or((String::new(), 0.0), |m| (h.w.names.str(m.montage.name).to_owned(), m.pos))
}

/// A frame of the baseline's `name` where fighter `n` is on the floor inside BranchWindow_C_0, before the Immediate
/// BranchWindow_C_1.
fn find(name: &str, n: usize) -> Option<usize> {
    let t = mvs_trace::Trace::read(&run::baseline().dir.join(format!("{name}.trace"))).ok()?;
    (0..t.len()).find(|&i| {
        let fr = t.frame(i).expect("a frame of the trace");
        let x = fr.world.fighters.get(n);
        x.is_some_and(|x| {
            x.on_floor
                && x.montage.as_ref().is_some_and(|m| m.name == MONTAGE && m.position > 1.19 && m.position < 1.36)
        })
    })
}

#[test]
fn a_stored_press_for_the_stopped_attack_itself_starts_nothing() {
    let found = MATCHES.iter().flat_map(|m| [0, 1].map(|n| (*m, n))).find_map(|(m, n)| find(m, n).map(|fr| (m, n, fr)));
    let Some((name, n, frame)) = found else { return };
    let mut h = harness::at_baseline(name, frame).expect("the frame loads");
    let f = h.fighter(n);
    assert_eq!(montage_of(&h, f).0, MONTAGE);
    // no stored press, and no window taken out by an earlier trigger
    h.w.fighter_mut(f).branching.queued = None;
    h.w.fighter_mut(f).branching.win_cleared = None;
    assert!(
        buffer_press(&mut h.w, f, QueuedBtn::Press(Btn::Attack), Dir::Up, None, None),
        "the window takes the press"
    );
    // a stop before the frame's dispatch, on the floor
    h.w.fighter_mut(f).root.anim_updated = false;
    stop_montage(&mut h.w, f, None, None);
    assert!(h.w.fighter(f).branching.late_branch.is_some(), "the window is filed for its late end");
    for _ in 0..4 {
        h.step(&[NO_INPUT, NO_INPUT]);
        let (m, pos) = montage_of(&h, f);
        assert!(!(m == MONTAGE && pos < 0.5), "the stopped attack starts again: {m} at {pos}");
    }
}
