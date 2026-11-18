//! The per-tick trace of a match: the oracle's node run writes one per
//! match, the Rust sim the same record from its World, and `trace-diff` names the first tick and field that differ.

pub mod diff;
pub mod field;
pub mod file;
pub mod record;
pub mod stable;
pub mod value;

pub use diff::{Outcome, compare, compare_kept, diff_frames};
pub use field::{Difference, Field};
pub use file::{Header, Trace, TraceWriter};
pub use record::*;
pub use value::{EventRecord, Obj, Value};

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("the trace ends inside a value at byte {0}")]
    Short(usize),
    #[error("bad trace bytes at {0}: {1}")]
    Bad(usize, &'static str),
    #[error("the trace header is not valid: {0}")]
    Header(serde_json::Error),
    #[error("trace format {0}, this reader reads {fmt}", fmt = file::FORMAT)]
    Format(u32),
    #[error("the trace's schema differs from this reader's at leaf {at}: the file has {file:?}, the reader {ours:?}")]
    Schema { at: usize, file: String, ours: String },
    #[error("the header names {header} ticks, the file holds {frames}")]
    Ticks { header: u32, frames: usize },
}
