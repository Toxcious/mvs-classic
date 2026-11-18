//! Batcave's bat swarms (BatcaveBatsBP_C) drawn from their sim state (mvs-sim stages/batcave/bats.rs):
//! PS_BatcaveBats_Warning at `warn_at` (BatsWarningVisuals @2217), destroyed
//! 4.5 s later (Delay @2313 -> @15); PS_BatcaveBats_Full at `main_at` (BatsMainVisuals @2368), deactivated
//! BatsDuration later (@2464 -> @49) and destroyed 3 s after that (@85 -> @179). Both at BoxOfBatsBottom + (0, 0,
//! -1550), scale (1, 1, 1.2). The Delays as whole ticks of 1/60 s. PS_BatsClaws (PulseHitVFX @2600) only with
//! PlayHitVFX, false on both. A swarm's systems are spawned by a timer or an event: first ticked the next frame; a tick
//! stamp seen 6 ticks late or more (a rollback that played it again) spawns nothing.

use mvs_sim::stages::batcave::BatcaveBats;
use mvs_vfx::{Attachment, SystemId};
use serde::Deserialize;

use super::car::FX_LATE;
use crate::scenery::host::SceneryHost;
use crate::scenery::the_court::rows_of;

/// A swarm of the block's `bats`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BatsData {
    pub(super) actor: String,
    warn: String,
    full: String,
    loc: [f64; 3],
    rot: [f64; 3],
    scale: [f64; 3],
    warn_life: f64,
    duration: f64,
    kill_after: f64,
}

/// A swarm as it is drawn.
#[derive(Debug)]
pub(super) struct Bats {
    pub(super) d: BatsData,
    at: Attachment,
    warn: Option<SystemId>,
    warn_seen: i32,
    main: Option<SystemId>,
    main_seen: i32,
    main_off: bool,
    /// The Delays in ticks: the warning's life, the full swarm's till its deactivation and till its end.
    warn_life: i32,
    main_life: i32,
    main_kill: i32,
}

/// A whole number of ticks of `s` seconds (Math.round: these are whole and positive).
fn ticks(s: f64) -> i32 {
    (s * 60.0).round() as i32
}

impl Bats {
    pub(super) fn new(d: BatsData) -> Bats {
        Bats {
            at: Attachment { loc: d.loc, rot: rows_of(d.rot), scale: Some(d.scale) },
            warn: None,
            warn_seen: -1,
            main: None,
            main_seen: -1,
            main_off: false,
            warn_life: ticks(d.warn_life),
            main_life: ticks(d.duration),
            main_kill: ticks(d.duration + d.kill_after),
            d,
        }
    }

    /// The level begins: the systems of the level before are gone (VfxPlayer.endAll).
    pub(super) fn begin(&mut self) {
        self.warn = None;
        self.main = None;
        self.warn_seen = -1;
        self.main_seen = -1;
        self.main_off = false;
    }

    #[cfg(test)]
    pub(super) fn warn_alive(&self, v: &mvs_vfx::Player) -> bool {
        self.warn.is_some_and(|id| v.alive(id))
    }

    #[cfg(test)]
    pub(super) fn main_alive(&self, v: &mvs_vfx::Player) -> bool {
        self.main.is_some_and(|id| v.alive(id))
    }

    fn spawn(&self, host: &mut SceneryHost, system: &str) -> Option<SystemId> {
        host.vfx()?.spawn_level(system, &self.at, true, &[], 0, false)
    }

    fn kill(host: &mut SceneryHost, id: Option<SystemId>) {
        if let (Some(id), Some(v)) = (id, host.vfx()) {
            v.kill(id);
        }
    }

    /// One tick of the World's swarm `a`.
    pub(super) fn tick(&mut self, host: &mut SceneryHost, a: &BatcaveBats, tick: i32) {
        if a.warn_at != self.warn_seen {
            Bats::kill(host, self.warn.take());
            self.warn_seen = a.warn_at;
            if a.warn_at >= 0 && tick - a.warn_at < FX_LATE {
                self.warn = self.spawn(host, &self.d.warn);
            }
        }
        if self.warn.is_some() && tick - a.warn_at >= self.warn_life {
            Bats::kill(host, self.warn.take());
        }
        if a.main_at != self.main_seen {
            Bats::kill(host, self.main.take());
            self.main_seen = a.main_at;
            self.main_off = false;
            if a.main_at >= 0 && tick - a.main_at < FX_LATE {
                self.main = self.spawn(host, &self.d.full);
            }
        }
        if let Some(id) = self.main {
            let el = tick - a.main_at;
            if el >= self.main_kill {
                Bats::kill(host, self.main.take());
            } else if el >= self.main_life && !self.main_off {
                self.main_off = true;
                if let Some(v) = host.vfx() {
                    v.deactivate(id);
                }
            }
        }
    }
}
