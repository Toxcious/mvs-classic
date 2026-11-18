//! The import: the kinds built from the player's paks into the store.
//!
//! Modules:
//! - `kind`: the [`kind::Kind`] trait every kind implements, what its list and builds read ([`kind::Cx`]), items,
//!   errors, progress.
//! - `kinds`: every kind, entered once in `kinds::KINDS`.
//! - `catalog`: every package's top-level exports and their classes, the lists' input.
//! - `objview`: a build's object as the object kind reads it ([`kind::Cx::object`]), for the kinds that read
//!   objects; `reads`: the entries of the store's kinds a game kind reads ([`kind::Cx::store`]).
//! - `game`: what the game kinds share (versions, entry paths, the stages); `stage_levels`: a stage's levels as the
//!   kinds that draw it read them.
//! - `json`, `jsonbin`: entries as an earlier importer wrote them: JSON.stringify's JSON and the json-bin container.
//! - `run`: the import of every stale kind into a store; `lock`: one import on the machine at a time.

pub mod catalog;
pub mod fighter_port;
pub mod game;
pub mod json;
pub mod jsonbin;
pub mod kind;
pub mod kinds;
pub mod lock;
pub mod objview;
pub mod reads;
pub mod run;
pub mod stage_levels;
