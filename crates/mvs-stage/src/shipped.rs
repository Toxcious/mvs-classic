//! The layouts the game ships, compiled in: each is a map of the custom games, Local Play and the Lab, named
//! `map_custom_<key>` on the menus. A layout from disk (`mvs --stage-layout`) is a dev run's and never plays online.

use crate::Layout;

/// The shipped layouts' files.
pub const FILES: [&str; 1] = [include_str!("../layouts/rising.json")];

/// The menus' name of every shipped layout's map starts so (`map_custom_<key>`); the host lets in map names that
/// start `map_`.
pub const MAP_PREFIX: &str = "map_custom_";

/// The shipped layouts, in their files' order. Each reads and passes `check` (the crate's tests).
pub fn shipped() -> Vec<Layout> {
    FILES.iter().filter_map(|t| Layout::read(t).ok()).collect()
}

/// The menus' map name of the layout `key`.
pub fn map_name(key: &str) -> String {
    format!("{MAP_PREFIX}{key}")
}

/// Whether the menus' map `map` is a shipped layout's.
pub fn is_custom_map(map: &str) -> bool {
    map.starts_with(MAP_PREFIX)
}

/// The shipped layout the menus' map `map` plays, or None (a map of the build, or no such layout).
pub fn layout_of_map(map: &str) -> Option<Layout> {
    let key = map.strip_prefix(MAP_PREFIX)?;
    shipped().into_iter().find(|l| l.key == key)
}

/// The shipped layouts as one text, each in its written form: what two games must share to play a custom map
/// together (online's compatibility id holds its hash).
pub fn fingerprint() -> String {
    let each: Vec<String> = shipped().iter().map(|l| serde_json::to_string(l).unwrap_or_default()).collect();
    each.join("|")
}
