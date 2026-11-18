//! The release tool's parts: the guard over the zips a release is handed out in (`mvs_release::zip`), the pack step
//! that builds and writes a release, the local feed, and the release check (two test releases walked through an update).

pub mod check;
pub mod guard;
pub mod pack;
pub mod serve;
