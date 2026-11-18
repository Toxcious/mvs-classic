//! The bank merge's cost and result on the developer's store (opt-in: `MVS_BANK_COST=1`, `--nocapture`): the events
//! of two, then four, then another four fighters held as one scope in turn, the time of each hold and a digest of
//! the merge after it.

use std::collections::BTreeMap;
use std::time::Instant;

use super::bank::Banks;

/// The events whose names hold any of `keys`, sorted.
fn events_of(init: &mvs_data::sound::InitBank, keys: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = init.events.keys().filter(|e| keys.iter().any(|k| e.contains(k))).cloned().collect();
    v.sort();
    v
}

/// FNV-1a 64 of the merge's objects, events and streamed flags, in id order.
fn digest(b: &Banks, ids: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |s: &str| {
        for x in s.bytes() {
            h = (h ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for &id in ids {
        eat(&format!("{id}:{:?}|{:?}|{:?}|{}", b.obj(id), b.ev_obj(id), b.media(id).map(|m| &m.path), b.streamed(id)));
    }
    h
}

#[test]
fn hold_cost() {
    if std::env::var_os("MVS_BANK_COST").is_none() {
        eprintln!("hold_cost: MVS_BANK_COST unset, nothing measured");
        return;
    }
    let store = mvs_store::Store::open(mvs_store::dev_store_dir()).expect("the dev store");
    let init = mvs_data::sound::load_init(&store).expect("the InitBank");
    let sets: [(&str, &[&str]); 4] = [
        ("2 fighters", &["shaggy", "batman"]),
        ("4 fighters", &["shaggy", "batman", "wonderwoman", "harley"]),
        ("4 others", &["finn", "jake", "taz", "superman"]),
        ("2 fighters again", &["shaggy", "batman"]),
    ];
    let mut b = Banks::default();
    let mut all_ids: BTreeMap<u32, ()> = BTreeMap::new();
    for (what, keys) in sets {
        let names = events_of(&init, keys);
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let banks = super::scope::banks(&store, &init, &refs).expect("banks");
        let n = banks.len();
        for (_, bank) in &banks {
            all_ids.insert(bank.id, ());
            for id in bank.objs.keys().chain(bank.posted.keys()).chain(bank.media.keys()) {
                all_ids.insert(*id, ());
            }
        }
        let t0 = Instant::now();
        b.hold(7, banks);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let t1 = Instant::now();
        let v = b.view();
        let view_ms = t1.elapsed().as_secs_f64() * 1e3;
        drop(v);
        let ids: Vec<u32> = all_ids.keys().copied().collect();
        let d = digest(&b, &ids);
        eprintln!(
            "hold_cost: {what}: {} events, {n} banks, hold {ms:.1} ms, view {view_ms:.1} ms, digest {d:016x}",
            names.len()
        );
    }
    let t0 = Instant::now();
    b.unhold(7);
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let ids: Vec<u32> = all_ids.keys().copied().collect();
    eprintln!("hold_cost: unhold {ms:.1} ms, digest {:016x}", digest(&b, &ids));
}
