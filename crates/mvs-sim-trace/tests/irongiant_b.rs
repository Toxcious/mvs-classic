//! Paths of Iron Giant's perks that the baseline matches reach only through another track's actors (the temp health
//! packs of his bolts and weapon mode): the World loaded at the frame before the oracle's add, the signal sent as the
//! Comp_Armor broadcasts it, and the buff's extension compared with the oracle's next frame.

use mvs_sim::combat::buffs::buff_signal;
use mvs_sim::registry::signals::{BuffSignal, BuffSignalArg};
use mvs_sim_trace::harness;

/// The differences of the `ext` of the buff at `path` between the oracle's frame `n` and the harness's World now.
fn ext_diffs(h: &harness::Harness, n: usize, path: &str) -> Vec<String> {
    mvs_trace::diff_frames(&h.recorded(n), &h.record())
        .into_iter()
        .filter(|d| d.path.starts_with(path))
        .map(|d| format!("{}: oracle {} | Rust {}", d.path, d.expected, d.actual))
        .collect()
}

fn pack(h: &mut harness::Harness, f: mvs_sim::fighter::FighterId, name: &str, sig: BuffSignal) {
    let n = h.w.names.get(name).expect("the pack's name is in the data");
    buff_signal(&mut h.w, f, sig, BuffSignalArg { pack: Some(n), ..BuffSignalArg::default() });
}

/// TempHealthAdded of the first type on a teammate with the marker: Buff_Thorns spawned and kept (`thorns`), the type
/// listed; the second type adds a stack and `thorns` goes none (the build's out parameter).
#[test]
fn damage_reflect_adds_and_stacks() {
    let name = "irongiant-irongiant";
    let Some(mut h) = harness::at(name, 444) else { return };
    let f = h.fighter(2);
    pack(&mut h, f, "C017_AllyBurst", BuffSignal::TempHealthAdded);
    let d = ext_diffs(&h, 445, "world.fighters[2].buffs[0].ext");
    assert!(d.is_empty(), "first type:\n{}", d.join("\n"));

    let Some(mut h) = harness::at(name, 495) else { return };
    let f = h.fighter(2);
    pack(&mut h, f, "IGBoltTempHealth", BuffSignal::TempHealthAdded);
    let d = ext_diffs(&h, 496, "world.fighters[2].buffs[0].ext");
    assert!(d.is_empty(), "second type:\n{}", d.join("\n"));
}
