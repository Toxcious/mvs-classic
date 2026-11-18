//! The tests' links and clock: players in one process, a time the test moves on, no socket and no
//! sleep.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use mvs_host::{Connection, Core, HostWhy};

/// A clock that stands still until `advance(ms)`, which pumps the core at every deadline due by then in time order.
pub struct ManualClock {
    pub t: f64,
}

impl ManualClock {
    /// The time moves on by `ms`, each deadline due on the way run at its own time (a timer's code reads the clock as
    /// its due time).
    pub fn advance(&mut self, core: &mut Core, ms: f64) {
        let end = self.t + ms;
        while let Some(at) = core.next_due().filter(|&at| at <= end) {
            self.t = self.t.max(at);
            core.pump(self.t);
            if core.ended() {
                break;
            }
        }
        self.t = end;
    }
}

/// What a memory link saw: the core's messages (JSON) and its close. `throws`: a test's broken link, whose send fails.
#[derive(Default)]
pub struct PeerLog {
    pub got: Vec<serde_json::Value>,
    pub closed_why: Option<HostWhy>,
    pub throws: bool,
}

/// A player on an in-memory link to a core: the test keeps the log, the core the link.
pub struct MemoryPeer {
    pub id: String,
    pub name: String,
    pub log: Arc<Mutex<PeerLog>>,
}

impl MemoryPeer {
    /// A link of `id` and its log.
    pub fn new(id: &str, name: &str) -> (MemoryPeer, Arc<Mutex<PeerLog>>) {
        let log = Arc::new(Mutex::new(PeerLog::default()));
        (MemoryPeer { id: id.to_string(), name: name.to_string(), log: Arc::clone(&log) }, log)
    }
}

impl Connection for MemoryPeer {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn send(&mut self, text: &str) -> Result<(), String> {
        let mut log = self.log.lock().expect("a peer's log");
        if log.throws {
            return Err("Error: broken link".to_string());
        }
        if log.closed_why.is_none() {
            log.got.push(serde_json::from_str(text).expect("the core sends JSON"));
        }
        Ok(())
    }

    fn close(&mut self, why: HostWhy) {
        let mut log = self.log.lock().expect("a peer's log");
        if log.closed_why.is_none() {
            log.closed_why = Some(why);
        }
    }
}
