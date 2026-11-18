//! Every frame of every baseline trace loaded into a World and written back. A frame that does not come back byte
//! for byte fails the test only through a path outside `run::GAPS` (the fields whose Rust form cannot hold the
//! record's value yet, each naming the Rust field it waits on); a miss outside `run::GAP_MISSES` and an unwritten
//! value outside `run::GAP_UNMAPPED` fail it too. With no traces or no store the test passes with a note. A trace whose
//! header names another store runs all the same with a note (the baselines carry an older fighter kind than the
//! stores, as every `sim-trace` run's `--ignore-header`); a run that checks no trace at all fails.

use mvs_sim_trace::run;

/// The traces of a character whose port is under way: its tracks map their own state into the record, and the
/// batch's last merge empties this list.
// Bugs Bunny's, Rick's, Finn's and Stripe's twelve: their ports.
const IN_PORT: &[&str] = &[
    "bugs-shaggy",
    "bugs-bugs",
    "bugs-combat",
    "rick-shaggy",
    "rick-rick",
    "rick-combat",
    "finn-shaggy",
    "finn-finn",
    "finn-combat",
    "stripe-shaggy",
    "stripe-stripe",
    "stripe-combat",
];

#[test]
fn round_trip_of_the_baselines() {
    let dir = run::baseline().dir;
    let list = match run::traces(&dir, &[]) {
        Ok(l) if !l.is_empty() => l,
        _ => {
            eprintln!("note: no traces in {} (the oracle's trace run writes them); nothing checked", dir.display());
            return;
        }
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store at {}; nothing checked", mvs_store::dev_store_dir().display());
        return;
    };
    let mut unknown = Vec::new();
    let mut checked = 0;
    for (name, path) in &list {
        if IN_PORT.contains(&name.as_str()) {
            eprintln!("note: {name} is a character's trace still being ported; left out");
            continue;
        }
        let t = mvs_trace::Trace::read(path).expect("a trace that reads");
        let header = mvs_sim_trace::store_differences(&t.header, &store);
        if !header.is_empty() {
            eprintln!("note: {name} names another store ({}); run all the same", header.join("; "));
        }
        let rt = run::round_trip(&t, &store, None).expect("the World of the header");
        checked += 1;
        eprintln!("{name}: {} of {} frames byte-equal", rt.equal, rt.frames);
        for p in rt.paths.keys().filter(|p| !run::is_gap(p)) {
            unknown.push(format!("{name}: differs {p}"));
        }
        for m in rt.misses.keys().filter(|m| !run::is_gap_miss(m)) {
            unknown.push(format!("{name}: not loaded {m}"));
        }
        for u in rt.unmapped.keys().filter(|u| !run::GAP_UNMAPPED.contains(&u.as_str())) {
            unknown.push(format!("{name}: not written {u}"));
        }
    }
    assert!(checked > 0, "no trace of {} checked (all {} in IN_PORT)", dir.display(), list.len());
    assert!(unknown.is_empty(), "outside the known gaps:\n{}", unknown.join("\n"));
}
