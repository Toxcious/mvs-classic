//! The loader: a few worker threads below normal priority that build what a load needs while the frame thread goes
//! on drawing. A load started with [`spawn`] gives a [`Ticket`] the frame thread polls each frame
//! ([`Ticket::try_take`]) and installs when it is ready; the parts of a load that split ([`map`]: a decode per
//! medium) run on every worker at once, the caller's thread among them when it is one.
//!
//! What a job builds is plain owned data (`Send`); the shared results (the store's cache, the decoded sound) are
//! kept by their own caches, so a job asked twice costs a lookup the second time.

use std::sync::OnceLock;
use std::sync::mpsc;

/// The loader's pool, made on first use: one thread per core but two (the frame thread and the sound's mixer keep
/// theirs), at least two and at most eight, each below normal priority.
fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores.saturating_sub(2).clamp(2, 8))
            .thread_name(|i| format!("mvs-load-{i}"))
            .start_handler(|_| below_normal())
            .build()
            .expect("the loader's threads")
    })
}

/// The loader's thread count.
pub fn threads() -> usize {
    pool().current_num_threads()
}

#[cfg(windows)]
fn below_normal() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    // SAFETY: the calling thread's own pseudo-handle and a valid priority; a failure leaves it at normal priority.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

#[cfg(not(windows))]
fn below_normal() {}

/// A load running on the loader: its value once built.
#[derive(Debug)]
pub struct Ticket<T> {
    rx: mpsc::Receiver<std::thread::Result<T>>,
    done: Option<T>,
}

impl<T> Ticket<T> {
    /// The value if the load is done (taken: a second call gives None). A load that panicked panics here, on the
    /// thread that asked for it.
    pub fn try_take(&mut self) -> Option<T> {
        if let Some(v) = self.done.take() {
            return Some(v);
        }
        match self.rx.try_recv() {
            Ok(r) => Some(r.unwrap_or_else(|p| std::panic::resume_unwind(p))),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => None,
        }
    }

    /// Whether [`Ticket::try_take`] would give the value now.
    pub fn is_ready(&mut self) -> bool {
        if self.done.is_none()
            && let Ok(r) = self.rx.try_recv()
        {
            self.done = Some(r.unwrap_or_else(|p| std::panic::resume_unwind(p)));
        }
        self.done.is_some()
    }

    /// Waits for the value (a load the caller cannot go on without).
    pub fn wait(mut self) -> T {
        if let Some(v) = self.done.take() {
            return v;
        }
        self.rx.recv().expect("a load's ticket taken twice").unwrap_or_else(|p| std::panic::resume_unwind(p))
    }
}

/// Starts `job` on the loader. A ticket dropped before the job ends lets its value go when it ends (what it put in
/// the caches stays).
pub fn spawn<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Ticket<T> {
    let (tx, rx) = mpsc::sync_channel(1);
    pool().spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
        let _ = tx.send(r);
    });
    Ticket { rx, done: None }
}

/// `f` over `items` on every loader thread, the results in the items' order. Called from a loader thread it joins
/// the work; from any other thread it waits for it.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    use rayon::prelude::*;
    if items.len() < 2 {
        return items.iter().map(f).collect();
    }
    pool().install(|| items.par_iter().map(f).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_gives_its_value_once() {
        let mut t = spawn(|| 6 * 7);
        let v = loop {
            if let Some(v) = t.try_take() {
                break v;
            }
            std::thread::yield_now();
        };
        assert_eq!(v, 42);
        assert_eq!(spawn(|| "x").wait(), "x");
        let mut t = spawn(|| 1);
        while !t.is_ready() {
            std::thread::yield_now();
        }
        assert_eq!(t.try_take(), Some(1));
        assert_eq!(t.try_take(), None);
    }

    #[test]
    fn map_keeps_the_order() {
        let v: Vec<u32> = (0..1000).collect();
        assert_eq!(map(&v, |x| x * 2), v.iter().map(|x| x * 2).collect::<Vec<_>>());
        // (from a loader thread: the nested map joins the work)
        let nested = spawn(move || map(&[1u32, 2, 3], |x| x + 1)).wait();
        assert_eq!(nested, [2, 3, 4]);
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn a_panic_reaches_the_asker() {
        spawn(|| -> u8 { panic!("boom") }).wait();
    }
}
