//! Two traces of one match compared: the first tick whose frames differ and the fields that differ in it.

use crate::TraceError;
use crate::field::{Difference, Field};
use crate::file::Trace;
use crate::record::Frame;

pub enum Outcome {
    Same {
        ticks: usize,
    },
    /// Frame `tick` (its index in the file) differs: every differing field of it, in write order.
    Differs {
        tick: usize,
        fields: Vec<Difference>,
    },
    /// Every frame of the shorter trace is the same; the other has more.
    Length {
        expected: usize,
        actual: usize,
    },
}

/// The differing fields of two frames, in write order.
pub fn diff_frames(a: &Frame, b: &Frame) -> Vec<Difference> {
    let mut out = Vec::new();
    Frame::diff(a, b, &mut String::new(), &mut out);
    out
}

pub fn compare(expected: &Trace, actual: &Trace) -> Result<Outcome, TraceError> {
    compare_kept(expected, actual, |_| true)
}

/// `compare` counting only the differences `keep` keeps (`stable::keeps`: the stage-B class).
pub fn compare_kept(
    expected: &Trace,
    actual: &Trace,
    keep: impl Fn(&Difference) -> bool,
) -> Result<Outcome, TraceError> {
    let n = expected.len().min(actual.len());
    for i in 0..n {
        if expected.frame_bytes(i) == actual.frame_bytes(i) {
            continue;
        }
        let mut fields = diff_frames(&expected.frame(i)?, &actual.frame(i)?);
        fields.retain(&keep);
        // Bytes that differ with no field that does: only another NaN's bits, which the writers make one (or, with
        // a filter, only fields it drops).
        if fields.is_empty() {
            continue;
        }
        return Ok(Outcome::Differs { tick: i, fields });
    }
    if expected.len() != actual.len() {
        return Ok(Outcome::Length { expected: expected.len(), actual: actual.len() });
    }
    Ok(Outcome::Same { ticks: n })
}
