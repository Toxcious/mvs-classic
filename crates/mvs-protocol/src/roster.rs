//! The port's playable characters by key: the id list the server picks from (a pre-match's pick,
//! the queue's bots) and every part of the program names its fighters by.

/// The characters by key, in roster order.
pub const CHARACTERS: [&str; 23] = [
    "shaggy",
    "morty",
    "irongiant",
    "wonderwoman",
    "harley",
    "superman",
    "lebron",
    "arya",
    "batman",
    "creature",
    "blackadam",
    "bugs",
    "rick",
    "taz",
    "steven",
    "garnet",
    "finn",
    "jake",
    "tomandjerry",
    "velma",
    "marvin",
    "stripe",
    "gizmo",
];

/// The character a match falls back on.
pub const DEFAULT_CHARACTER: &str = "shaggy";
