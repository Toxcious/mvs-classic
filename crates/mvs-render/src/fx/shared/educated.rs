//! Buff_Educated's end burst: a shared class's effects,
//! made for every match after the characters'. Ported from the decode (Buff_Educated.txt, rotation and Team read with
//! kismet_pseudo --extra).
//!
//! RemoveBuffVisuals (@542, from ReceiveEndPlay @2837): each ParticleSystems component destroyed (the class spawns
//! none: the parent's list), then with a BaseFighter owner @1399-1520: SpawnEmitterAttached(
//! PS_WonderWomanBracers_Cancel, the owner's Mesh, fml_un_C_head, 0, rot 0, (0.75, 0.75, 0.75) / Mesh.RelativeScale3D
//! @1808, KeepRelativeOffset, auto destroy) @1945, Team = 0.6 for a local online player, else the owner's ColorIndex
//! @2259 (`Fx::team`). On a spawned actor's Comp_State_Manager the owner casts fail and nothing is spawned (the
//! sim's `educated_actor_end` posts the sound alone). Buff_Educated_C020 is its child with no graph of its own.
//! The sim has no event for it: the end is seen as the buff entry (by its serial) leaving the fighter's list while
//! the fighter is in the world.
// Unverified: an entry whose frames ran out is taken as ended at that step (the sim's list keeps an ended entry until
// its sweep, `buffs::frames` reads 0 for it).
// Deviation: a Lab reset drops the tracked entries without a burst (the reset's World has no buffs; the build's reset
// destroys the buff actors, whose EndPlay would spawn it on the old pawn).

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_vfx::math::IDENTITY;

use crate::fx::Fx;
use crate::fx::chars::{CharFxHooks, ProjRule};

const CANCEL: &str = "PS_WonderWomanBracers_Cancel";
const SOCKET: &str = "fml_un_C_head";
const SCALE: f64 = 0.75;

/// What its effects keep between steps: per fighter (id), the serials of its live Educated entries.
#[derive(Debug, Default)]
pub(crate) struct Educated {
    live: IndexMap<i32, Vec<u32>>,
}

/// Whether a buff entry is a live Educated.
fn live(class: BuffClass, frames: f32) -> bool {
    matches!(class, BuffClass::BuffEducated | BuffClass::BuffEducatedC020) && frames > 0.0
}

impl CharFxHooks for Educated {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let fighters = w.pawns.as_deref().unwrap_or(&w.players);
        for &f in fighters {
            let (id, list) = (w.fighter(f).id, &w.fighter(f).buffs.buffs);
            let ended = self.live.get(&id).map_or(0, |was| {
                was.iter().filter(|s| !list.iter().any(|b| b.serial == **s && live(b.class, b.frames))).count()
            });
            for _ in 0..ended {
                let team = fx.team(id);
                let s = SCALE / fx.mesh_relative_scale(w, id);
                fx.attach_socket(w, id, CANCEL, SOCKET, [0.0; 3], &IDENTITY, [s; 3], &[("Team", team)]);
            }
            if list.iter().any(|b| live(b.class, b.frames)) {
                let now = self.live.entry(id).or_default();
                now.clear();
                now.extend(list.iter().filter(|b| live(b.class, b.frames)).map(|b| b.serial));
            } else {
                self.live.shift_remove(&id);
            }
        }
        // (a fighter gone from the world takes its components with it)
        self.live.retain(|id, _| fighters.iter().any(|&f| w.fighter(f).id == *id));
    }

    fn reset(&mut self, _fx: &mut Fx) {
        self.live.clear();
    }

    fn end(&mut self, _fx: &mut Fx) {
        self.live.clear();
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
