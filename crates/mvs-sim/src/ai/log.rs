//! The bot log: what the bots' trees did, think by think, for a tool that compares the port's bots with a recorded
//! run's (sim-trace `--bot-log`; the recorded run's replay writes the same lines). Off unless a tool starts it on its
//! thread; it only reads, so the sim steps the same with it on.
//!
//! The lines, in the order the runner makes them:
//! - `think <who>`: a controller's tick starts; `who` is `p<i>` for the player in slot i, `m<j>` for the j-th pawn of
//!   the match's pawn list, `?` for neither;
//! - `exec <node id>`: a task executes; `abort <node id>`: the running task is aborted; `tree finished`;
//! - `dec <decorator id> <0|1>`: a decorator's condition check and its result (inverse applied);
//! - `svc <service id> <ticks>`: a service's next tick scheduled, that many ticks away.

use std::cell::RefCell;

thread_local! {
    static SINK: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Starts the log on this thread (an earlier one's lines are dropped).
pub fn start() {
    SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
}

/// Stops the log on this thread.
pub fn stop() {
    SINK.with(|s| *s.borrow_mut() = None);
}

/// The lines since the last take (none when the log is off).
pub fn take() -> Vec<String> {
    SINK.with(|s| s.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
}

/// The log is on on this thread.
pub(crate) fn on() -> bool {
    SINK.with(|s| s.borrow().is_some())
}

/// A line, made only while the log is on.
pub(crate) fn line(ev: impl FnOnce() -> String) {
    SINK.with(|s| {
        if let Some(l) = s.borrow_mut().as_mut() {
            l.push(ev());
        }
    });
}

/// The log held off on this thread while the value lives: the Expert's look-ahead, whose trees think in a scratch
/// World, writes none of its thinks (`ai::expert`). Dropping it puts the log back as it was.
pub(crate) struct Quiet(Option<Vec<String>>);

/// Holds the log off until the [`Quiet`] is dropped.
pub(crate) fn quiet() -> Quiet {
    Quiet(SINK.with(|s| s.borrow_mut().take()))
}

impl Drop for Quiet {
    fn drop(&mut self) {
        if let Some(l) = self.0.take() {
            SINK.with(|s| *s.borrow_mut() = Some(l));
        }
    }
}
