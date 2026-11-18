//! Velma's perks: Buff_SpawnEvidence (the on-hit row of her camera flash and
//! sass bubble: a chance of a clue), Buff_Studied ([`StudiedExt`]: a clue counted while she has none), and the inert
//! markers Buff_KnowledgeIsPower (read by evidence.rs's pickup), Buff_Geometric and Buff_GoodReception (entered, they
//! do nothing).

use super::evidence::{EVIDENCE_N, evidence_count, increment_evidence, range_f, spawn_evidence_actor};
use crate::Real;
use crate::State;
use crate::combat::buffs::{BuffExt, buff_mut, end_buff};
use crate::combat::{BuffClass, BuffRef};
use crate::fighter::common::latent_frames;
use crate::fighter::{FighterId, actor_y};
use crate::math::to_int32;
use crate::registry::entries::BuffHooks;
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;

pub const SPAWN_EVIDENCE: &str = "Buff_SpawnEvidence_C";
pub const STUDIED: &str = "Buff_Studied_C";
pub const KNOWLEDGE_IS_POWER: &str = "Buff_KnowledgeIsPower_C";
pub const GEOMETRIC: &str = "Buff_Geometric_C";
pub const GOOD_RECEPTION: &str = "Buff_GoodReception_C";

/// Buff_Studied's extension (`StudiedState`: `{ wait }`).
#[derive(State, Debug, Default)]
pub struct StudiedExt {
    /// Ticks left of Delay(0.2) @921, 0: not running.
    pub wait: i32,
}

fn studied_of(w: &mut World, b: BuffRef) -> Option<&mut StudiedExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::Studied(e) => Some(e),
        _ => None,
    }
}

/// Buff_Studied @15-921 on f's buff: true when it applied.
fn studied(w: &mut World, f: FighterId) -> bool {
    match evidence_count(w, f) {
        Some(n) if n <= 0 => {
            increment_evidence(w, f, 1); // @847
            true
        }
        _ => false,
    }
}

// Unverified: RandomBoolWithWeight (FRand() <= weight) is drawn from the world's stream (2^24 steps), as
// chars/garnet/groove.rs; the three range draws are the world's too, in the pseudo's order.
fn spawn_evidence_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    if w.random_int(16777216.0) / 16777216.0 <= 0.35 {
        let _max = range_f(w, -2250.0, 2250.0); // the spawn's MaxSpeed: no reader for this class
        let dx = range_f(w, -1.0, 1.0);
        let dy = range_f(w, -1.0, 1.0);
        if let Some(from) = from {
            let (x, y) = (w.fighter(f).body.x, actor_y(w, f));
            spawn_evidence_actor(w, from, EVIDENCE_N, x, y, dx, dy, 2250.0, 0.2); // @631
        }
    }
    end_buff(w, f, b); // @749
}

fn studied_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let wait = if studied(w, f) { 0 } else { to_int32(latent_frames(0.2)) };
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Studied(StudiedExt { wait });
    }
}

fn studied_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = studied_of(w, b) else { return false };
    if e.wait <= 0 {
        return false;
    }
    e.wait -= 1;
    if e.wait > 0 {
        return false;
    }
    let wait = if studied(w, f) { 0 } else { to_int32(latent_frames(0.2)) };
    if let Some(e) = studied_of(w, b) {
        e.wait = wait;
    }
    false
}

/// This file's registry entries: Buff_SpawnEvidence_C, Buff_Studied_C, and Buff_KnowledgeIsPower_C, Buff_Geometric_C
/// and Buff_GoodReception_C with no hooks.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        SPAWN_EVIDENCE,
        BuffEntry::new(
            BuffClass::BuffSpawnEvidence,
            BuffHooks { begin: Some(spawn_evidence_begin), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(
        STUDIED,
        BuffEntry::new(
            BuffClass::BuffStudied,
            BuffHooks { begin: Some(studied_begin), tick: Some(studied_tick), ..BuffHooks::default() },
        ),
    )?;
    r.buffs.enter(KNOWLEDGE_IS_POWER, BuffEntry::new(BuffClass::BuffKnowledgeIsPower, BuffHooks::default()))?;
    r.buffs.enter(GEOMETRIC, BuffEntry::new(BuffClass::BuffGeometric, BuffHooks::default()))?;
    r.buffs.enter(GOOD_RECEPTION, BuffEntry::new(BuffClass::BuffGoodReception, BuffHooks::default()))
}
