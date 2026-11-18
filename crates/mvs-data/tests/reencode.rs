//! Every entry of the developer store's gamedata pack (the oracle importer's), decoded into its typed record and
//! encoded again by `value::encode`, is byte-equal to the oracle's bytes: the native kind's writer gives the oracle's
//! entries from the same records. A store without the kind at the version read says so and passes.

use std::time::Instant;

#[test]
fn every_entry_reencodes_to_its_bytes() {
    let dir = mvs_store::dev_store_dir();
    let store = match mvs_store::Store::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no store to read ({e}): NOTHING CHECKED");
            return;
        }
    };
    if store.version(mvs_data::KIND).map(mvs_data::format_of) != Some(mvs_data::KIND_VERSION) {
        eprintln!(
            "{}: no gamedata kind of format {} (found version {:?}): NOTHING CHECKED",
            dir.display(),
            mvs_data::KIND_VERSION,
            store.version(mvs_data::KIND)
        );
        return;
    }
    let pack = store.kind(mvs_data::KIND).expect("the gamedata pack");
    let t = Instant::now();
    let mut bad = Vec::new();
    let mut n = 0;
    let mut total = 0;
    for path in pack.paths() {
        let bytes = pack.bytes(path).expect("an entry's bytes");
        n += 1;
        total += bytes.len();
        match mvs_data::reencode_entry(path, &bytes) {
            Ok(b) if b == bytes => {}
            Ok(b) => {
                let at = b.iter().zip(&bytes).position(|(x, y)| x != y).unwrap_or(b.len().min(bytes.len()));
                let around =
                    |s: &[u8]| String::from_utf8_lossy(&s[at.saturating_sub(60)..(at + 60).min(s.len())]).into_owned();
                bad.push(format!(
                    "{path}: {} bytes, the oracle's {}; first difference at {at}:\n  ours:   {}\n  oracle: {}",
                    b.len(),
                    bytes.len(),
                    around(&b),
                    around(&bytes)
                ));
            }
            Err(e) => bad.push(format!("{path}: {e}")),
        }
    }
    eprintln!(
        "{}: {n} gamedata entries ({:.1} MB) re-encoded in {:.1} s, {} differ",
        dir.display(),
        total as f64 / 1048576.0,
        t.elapsed().as_secs_f64(),
        bad.len()
    );
    assert!(n > 0, "an empty gamedata pack");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
