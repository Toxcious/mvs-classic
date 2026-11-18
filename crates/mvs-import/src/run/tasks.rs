//! The kinds' lists cut into the build tasks: the items of one source package go together, whatever their kind (the
//! package is read once, its exports' objects once, and the cache may let it go after). The packages of
//! [`LARGE`] bytes or more (in the paks: no cost table) run first, the largest first, so none starts last and runs
//! alone; the rest in the paks' order (the cook's, which puts a package near those it was cooked with), cut into one
//! stripe per thread and dealt out in turn, so the threads work in as many places of the paks at once.
//!  One process shares its package cache, so
//! neighbouring packages need not be joined to share a worker's.
//!
//! Why stripes: the threads all on one run of neighbouring packages (the paks' order dealt out one by one, or the
//! largest first) hold far more memory than spread over the paks: the object kind at 16 threads peaked at 4.9 GB
//! in the paks' order and 4.6 GB largest first, against 1.6 GB spread, and ran 39 s largest first against 13 s
//! (the Wwise media packages, 140,000 in a row in the paks, hold more than the package cache counts).

use indexmap::IndexMap;
use mvs_pak::paths::PACKAGE_EXTENSIONS;
use mvs_pak::{BuildPaths, PakSet};

use crate::kind::Item;

/// One package's items across the kinds: (kind's place in the run, item's place in its list).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub package: String,
    /// The package's bytes in the paks (.uasset or .umap, .uexp, .ubulk); 0 for a path of no package.
    pub bytes: u64,
    pub items: Vec<(usize, usize)>,
}

/// Where a package's header lies in the paks: its pak's place in the set and its offset there.
type Place = Option<(usize, u64)>;

/// A package's [`Place`] and its bytes, by its object path.
fn place(paks: &PakSet, paths: &BuildPaths, package: &str) -> (Place, u64) {
    let Some(file) = paths.package_file(package) else { return (None, 0) };
    let mut at = None;
    let mut bytes = 0;
    for x in PACKAGE_EXTENSIONS.iter().chain(&[".uexp", ".ubulk"]) {
        if let Some(e) = paks.entry(&format!("{file}{x}")) {
            if at.is_none() && PACKAGE_EXTENSIONS.contains(x) {
                at = Some((e.pak, e.offset));
            }
            bytes += e.uncompressed_size;
        }
    }
    (at, bytes)
}

/// A package this large or larger runs before the rest.
pub const LARGE: u64 = 16 << 20;

/// The tasks of the lists (one per kind, in the run's kind order) in the order they run: the [`LARGE`] packages
/// largest first, then the rest in pak order (the packages without a place first, in the order first met) dealt out
/// from `stripes` stripes in turn.
pub fn cut(lists: &[&[Item]], paks: &PakSet, paths: &BuildPaths, stripes: usize) -> Vec<Task> {
    let mut groups: IndexMap<String, Vec<(usize, usize)>> = IndexMap::new();
    for (k, list) in lists.iter().enumerate() {
        for (i, item) in list.iter().enumerate() {
            groups.entry(item.package().to_string()).or_default().push((k, i));
        }
    }
    // (the header's place in the paks, the order first met, the task)
    let mut tasks: Vec<(Place, usize, Task)> = groups
        .into_iter()
        .enumerate()
        .map(|(n, (package, items))| {
            let (at, bytes) = place(paks, paths, &package);
            (at, n, Task { package, bytes, items })
        })
        .collect();
    tasks.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => a.1.cmp(&b.1),
    });
    let (mut large, rest): (Vec<Task>, Vec<Task>) = tasks.into_iter().map(|t| t.2).partition(|t| t.bytes >= LARGE);
    // (stable: equal sizes keep pak order)
    large.sort_by_key(|t| std::cmp::Reverse(t.bytes));
    let stripes = stripes.clamp(1, rest.len().max(1));
    let len = rest.len().div_ceil(stripes);
    let mut cut: Vec<std::vec::IntoIter<Task>> = Vec::with_capacity(stripes);
    let mut rest = rest.into_iter();
    for _ in 0..stripes {
        cut.push(rest.by_ref().take(len).collect::<Vec<_>>().into_iter());
    }
    let mut out = large;
    loop {
        let before = out.len();
        out.extend(cut.iter_mut().filter_map(Iterator::next));
        if out.len() == before {
            return out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripes_dealt_in_turn() {
        let paks = PakSet::open(&[], &mvs_pak::AesKey([0; 32])).unwrap();
        let paths = BuildPaths::new("MultiVersus", &[]).unwrap();
        let items: Vec<Item> = (0..7).map(|i| Item::new(format!("/Game/P{i}.P{i}"))).collect();
        let order = |n| -> Vec<String> { cut(&[&items], &paks, &paths, n).into_iter().map(|t| t.package).collect() };
        assert_eq!(order(1), ["/Game/P0", "/Game/P1", "/Game/P2", "/Game/P3", "/Game/P4", "/Game/P5", "/Game/P6"]);
        assert_eq!(order(3), ["/Game/P0", "/Game/P3", "/Game/P6", "/Game/P1", "/Game/P4", "/Game/P2", "/Game/P5"]);
        assert_eq!(order(100).len(), 7);
    }
}
