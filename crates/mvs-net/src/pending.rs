//! A call that waits on I/O (a connect, a join race, a probe, a listener's bind): it runs on a worker thread and the
//! service polls for its result, so no call on the service's thread waits. Dropping it cancels it (the flag the work
//! watches) and drops the result when it comes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

pub struct Pending<T> {
    rx: Receiver<T>,
    cancel: Arc<AtomicBool>,
}

impl<T: Send + 'static> Pending<T> {
    /// Runs `work` on a thread of its own; it is given the cancel flag and ends early when it is set.
    pub fn start(work: impl FnOnce(Arc<AtomicBool>) -> T + Send + 'static) -> Pending<T> {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work(flag));
        });
        Pending { rx, cancel }
    }
}

impl<T> Pending<T> {
    /// The result once it is there (once: later polls give None).
    pub fn poll(&mut self) -> Option<T> {
        // (an empty channel and a finished one both give none)
        self.rx.try_recv().ok()
    }

    /// Asks the work to end; its result (a cancelled failure) still comes.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Waits for the result: for tests and tools that have no service loop.
    pub fn wait(self) -> Option<T> {
        self.rx.recv().ok()
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}
