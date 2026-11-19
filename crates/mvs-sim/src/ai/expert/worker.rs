//! The Expert's decisions run beside the frame: a small pool of threads the caller makes ([`Workers`]) and hands to
//! its Expert bots (`BtBot::set_workers`). A decision is a pure function of its start (`job`), so it runs whole on a
//! worker and is taken back at the think the inline path takes it at (`t + REACTION - 1`): never earlier, and that
//! think waits for the worker if it is not done by then. What the bot gives each tick is the same as inline, bit for
//! bit; only the frame's thread is spared the work. The sim makes no thread of its own: without workers it runs inline.

use super::job::Job;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

type Task = Box<dyn FnOnce() + Send>;

/// The threads the Expert bots' decisions run on, shared by every Expert bot of a match (one job per bot at a time).
pub struct Workers {
    send: Mutex<Option<Sender<Task>>>,
    threads: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for Workers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workers").field("threads", &self.threads.len()).finish()
    }
}

impl Workers {
    /// A pool of `n` threads (at least one).
    pub fn new(n: usize) -> Workers {
        let (send, recv) = channel::<Task>();
        let recv = Arc::new(Mutex::new(recv));
        let threads = (0..n.max(1))
            .map(|i| {
                let recv = recv.clone();
                std::thread::Builder::new()
                    .name(format!("expert-{i}"))
                    .spawn(move || run(&recv))
                    .expect("an Expert worker thread")
            })
            .collect();
        Workers { send: Mutex::new(Some(send)), threads }
    }

    /// The pool this PC affords, `None` where it runs inline: one core leaves none to spare; else every core but
    /// two (the frame's thread and the renderer's or the sound's), at least one and at most `MAX_THREADS`.
    pub fn for_this_pc() -> Option<Workers> {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        (cores >= 2).then(|| Workers::new(cores.saturating_sub(2).clamp(1, MAX_THREADS)))
    }

    /// The pool's thread count.
    pub fn threads(&self) -> usize {
        self.threads.len()
    }

    fn spawn(&self, task: Task) {
        lock(&self.send).as_ref().expect("the pool is open").send(task).expect("the pool's threads are running");
    }
}

/// The most threads a pool takes: one per Expert bot of a four-player match.
pub const MAX_THREADS: usize = 4;

impl Drop for Workers {
    fn drop(&mut self) {
        lock(&self.send).take();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn run(recv: &Mutex<Receiver<Task>>) {
    loop {
        let task = lock(recv).recv();
        match task {
            Ok(task) => task(),
            Err(_) => return,
        }
    }
}

/// A lock whose holder panicked is taken all the same: what it guards is a count and a box handed over whole.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where a job on a worker is.
#[derive(Default)]
struct Progress {
    /// The job, back from the worker: finished, or stopped after `dropped`.
    back: Option<Box<Job>>,
    /// The worker's panic, where the job panicked.
    panic: Option<String>,
    /// The decision was given up: the worker stops at its next advance.
    dropped: bool,
}

#[derive(Default)]
struct Shared {
    progress: Mutex<Progress>,
    moved: Condvar,
}

/// A job sent to a worker.
pub(super) struct Ticket(Arc<Shared>);

impl Ticket {
    /// The job sent to `workers`: advanced there to its end, one think's share after the other (the same steps as
    /// inline).
    pub(super) fn send(workers: &Workers, mut job: Box<Job>) -> Ticket {
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        workers.spawn(Box::new(move || {
            let _quiet = crate::ai::log::quiet();
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                while !job.done() && !lock(&s.progress).dropped {
                    job.advance();
                }
                job
            }));
            let mut p = lock(&s.progress);
            match run {
                Ok(job) => p.back = Some(job),
                Err(e) => {
                    let text = e
                        .downcast_ref::<&str>()
                        .map(|s| (*s).to_owned())
                        .or_else(|| e.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "a panic".to_owned());
                    p.panic = Some(text);
                }
            }
            drop(p);
            s.moved.notify_all();
        }));
        Ticket(shared)
    }

    /// The job, back: waits for the worker to finish it, or to stop it if given up.
    pub(super) fn take(&self) -> Box<Job> {
        self.wait().back.take().expect("back")
    }

    /// A copy of the job once back (left for [`Ticket::take`]).
    pub(super) fn copy(&self) -> Box<Job> {
        self.wait().back.clone().expect("back")
    }

    /// The decision is given up: the worker stops at its next advance.
    pub(super) fn drop_job(&self) {
        lock(&self.0.progress).dropped = true;
    }

    fn wait(&self) -> MutexGuard<'_, Progress> {
        let mut p = lock(&self.0.progress);
        loop {
            if let Some(e) = &p.panic {
                panic!("the Expert's look-ahead panicked on its worker: {e}");
            }
            if p.back.is_some() {
                return p;
            }
            p = self.0.moved.wait(p).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.drop_job();
    }
}
