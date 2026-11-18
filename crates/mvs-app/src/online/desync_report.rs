//! (the port's own) The report of an online match that went out of sync: its set-up, the ruled inputs of every slot
//! and both hashes, written to the data folder's DESYNC_DIR for a bug report. Nothing is uploaded.

use std::path::{Path, PathBuf};

use mvs_protocol::match_wire::WireInput;
use mvs_protocol::setup::MatchSetupWire;
use serde::{Deserialize, Serialize};

/// The data folder's folder of desync reports.
pub const DESYNC_DIR: &str = "desyncs";
/// The reports kept: a write removes the oldest past this many.
pub const DESYNC_KEPT: usize = 20;

/// One desync's report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesyncReport {
    /// The game's version (mvs_release::VERSION) and the game data's compatibility id.
    pub version: String,
    pub compat: String,
    /// The match: its set-up as the World ran it (the sent perks, a fighter the port does not draw as the default
    /// character), its world seed, the map match.start named and the stage key the World loaded.
    pub setup: MatchSetupWire,
    pub seed: u32,
    pub map: String,
    pub stage: String,
    /// This player's slot.
    pub me: usize,
    /// The confirmed ticks the two hashes are of (mvs_net::session::Desync), the other peer's slot, and the hashes.
    pub tick: u32,
    pub slot: usize,
    pub mine: u32,
    pub theirs: u32,
    /// By slot, by tick from 0: the inputs the confirmed line ran (Session::on_ruled), as the wire encodes them.
    pub inputs: Vec<Vec<WireInput>>,
}

/// Writes `r` to a new file under `data`'s DESYNC_DIR, the oldest reports past DESYNC_KEPT removed: the file's path. Its name is the time it was written (s since
/// 1970) and the tick, one more number where that is taken.
pub fn write(data: &Path, r: &DesyncReport) -> std::io::Result<PathBuf> {
    let dir = data.join(DESYNC_DIR);
    std::fs::create_dir_all(&dir)?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let text = serde_json::to_vec(r).map_err(std::io::Error::other)?;
    for n in 0u32.. {
        let name = if n == 0 {
            format!("desync-{secs}-t{}.json", r.tick)
        } else {
            format!("desync-{secs}-t{}-{n}.json", r.tick)
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                std::io::Write::write_all(&mut file, &text)?;
                drop(file);
                prune(&dir, &path);
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!("a free name within u32::MAX tries")
}

/// The reports in `dir` past the newest DESYNC_KEPT (by their time of writing, `newest` the one just written)
/// removed; one that does not go stays.
fn prune(dir: &Path, newest: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut reports: Vec<(std::time::SystemTime, PathBuf)> = read
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("desync-") && n.ends_with(".json")
        })
        .map(|e| {
            let at = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (at, e.path())
        })
        .collect();
    // (newest first, the one just written whatever its clock says; ties by name)
    reports.sort_by(|a, b| (b.1 == newest).cmp(&(a.1 == newest)).then(b.0.cmp(&a.0)).then(b.1.cmp(&a.1)));
    for (_, p) in reports.into_iter().skip(DESYNC_KEPT) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_written_and_reads_back() {
        let data = std::env::temp_dir().join(format!("mvs-desync-report-{}", std::process::id()));
        let r = DesyncReport {
            version: "0.1.0".into(),
            compat: "c1".into(),
            setup: MatchSetupWire::default(),
            seed: 7,
            map: "m".into(),
            stage: "s".into(),
            me: 0,
            tick: 120,
            slot: 1,
            mine: 1,
            theirs: 2,
            inputs: vec![
                vec![[0; mvs_protocol::match_wire::INPUT_BYTES]; 3],
                vec![[1; mvs_protocol::match_wire::INPUT_BYTES]; 3],
            ],
        };
        let a = write(&data, &r).unwrap();
        let b = write(&data, &r).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.parent(), Some(data.join(DESYNC_DIR).as_path()));
        let back: DesyncReport = serde_json::from_slice(&std::fs::read(&a).unwrap()).unwrap();
        assert_eq!(back, r);
        for _ in 0..DESYNC_KEPT + 3 {
            write(&data, &r).unwrap();
        }
        assert_eq!(std::fs::read_dir(data.join(DESYNC_DIR)).unwrap().count(), DESYNC_KEPT);
        std::fs::remove_dir_all(&data).unwrap();
    }
}
